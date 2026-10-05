#include "include/playback.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#include <math.h>

typedef struct { float left, right; int64_t sample; uint64_t revision; } Frame;
struct KRAudioRing {
    Frame frames[KR_AUDIO_CAPACITY];
    _Atomic uint64_t read, write, stamp, host, underruns, missing, revision;
    _Atomic int64_t sample;
    _Atomic uint32_t callback_frames;
    _Atomic bool valid, timestamp_error;
    /* Consumer only; reset when both endpoints are quiescent. */
    int64_t origin, device_origin, device_end;
    uint64_t previous_host;
    bool started;
};
KRAudioRing *kr_audio_create(void) {
    KRAudioRing *r = calloc(1, sizeof(*r));
    if (!r) return NULL;
    atomic_init(&r->read, 0); atomic_init(&r->write, 0); atomic_init(&r->stamp, 0);
    atomic_init(&r->host, 0); atomic_init(&r->underruns, 0); atomic_init(&r->missing, 0);
    atomic_init(&r->revision, 0); atomic_init(&r->sample, 0); atomic_init(&r->callback_frames, 0);
    atomic_init(&r->valid, false); atomic_init(&r->timestamp_error, false);
    if (!atomic_is_lock_free(&r->read) || !atomic_is_lock_free(&r->sample) ||
        !atomic_is_lock_free(&r->callback_frames) || !atomic_is_lock_free(&r->valid)) {
        free(r); return NULL;
    }
    return r;
}
void kr_audio_free(KRAudioRing *r) { free(r); }
void kr_audio_reset(KRAudioRing *r, int64_t origin) {
    atomic_store(&r->read, 0); atomic_store(&r->write, 0);
    atomic_store(&r->valid, false); atomic_store(&r->timestamp_error, false);
    r->origin = origin; r->started = false;
}
uint32_t kr_audio_available(const KRAudioRing *r) {
    uint64_t w = atomic_load_explicit(&r->write, memory_order_relaxed);
    uint64_t rd = atomic_load_explicit(&r->read, memory_order_acquire);
    return KR_AUDIO_CAPACITY - (uint32_t)(w - rd);
}
bool kr_audio_push(KRAudioRing *r, const float *input, uint32_t n, int64_t start, uint64_t revision) {
    if (!input || !n || n > 4096 || n > kr_audio_available(r) || start < 0 || start > INT64_MAX - n) return false;
    uint64_t w = atomic_load_explicit(&r->write, memory_order_relaxed);
    for (uint32_t i = 0; i < n; i++) {
        r->frames[(w + i) % KR_AUDIO_CAPACITY] = (Frame){input[2*i], input[2*i+1], start + i, revision};
    }
    atomic_store_explicit(&r->write, w + n, memory_order_release);
    return true;
}
static void silence(AudioBufferList *b, uint32_t n) {
    for (uint32_t c = 0; c < b->mNumberBuffers; c++) {
        if (b->mBuffers[c].mData) memset(b->mBuffers[c].mData, 0, b->mBuffers[c].mDataByteSize);
    }
    (void)n;
}
void kr_audio_consume(KRAudioRing *r, const AudioTimeStamp *t, uint32_t n, AudioBufferList *b) {
    silence(b, n);
    if (!t || !n || n > KR_AUDIO_CAPACITY || !(t->mFlags & kAudioTimeStampSampleTimeValid) || !(t->mFlags & kAudioTimeStampHostTimeValid) ||
        !isfinite(t->mSampleTime) || t->mSampleTime < 0 || t->mSampleTime > 9007199254740991.0 ||
        floor(t->mSampleTime) != t->mSampleTime || b->mNumberBuffers != 2 ||
        b->mBuffers[0].mNumberChannels != 1 || b->mBuffers[1].mNumberChannels != 1 ||
        !b->mBuffers[0].mData || !b->mBuffers[1].mData ||
        b->mBuffers[0].mDataByteSize < (uint64_t)n * sizeof(float) || b->mBuffers[1].mDataByteSize < (uint64_t)n * sizeof(float)) {
        atomic_store_explicit(&r->timestamp_error, true, memory_order_relaxed); return;
    }
    int64_t device = (int64_t)t->mSampleTime;
    if (!r->started) { r->device_origin = device; r->device_end = device; r->previous_host = t->mHostTime; r->started = true; }
    if (device < r->device_end || t->mHostTime < r->previous_host) {
        atomic_store_explicit(&r->timestamp_error, true, memory_order_relaxed); return;
    }
    if (device < r->device_origin || r->origin > INT64_MAX - (device - r->device_origin) - n) {
        atomic_store_explicit(&r->timestamp_error, true, memory_order_relaxed); return;
    }
    int64_t start = r->origin + device - r->device_origin;
    r->device_end = device + n; r->previous_host = t->mHostTime;
    uint64_t rd = atomic_load_explicit(&r->read, memory_order_relaxed);
    uint64_t w = atomic_load_explicit(&r->write, memory_order_acquire);
    uint64_t revision = atomic_load_explicit(&r->revision, memory_order_relaxed);
    uint32_t missing = 0;
    float *left = b->mBuffers[0].mData, *right = b->mBuffers[1].mData;
    for (uint32_t i = 0; i < n; i++) {
        int64_t sample = start + i;
        /* Discard late blocks after underrun. Never play stale audio late. */
        while (rd < w && r->frames[rd % KR_AUDIO_CAPACITY].sample < sample) rd++;
        if (rd < w && r->frames[rd % KR_AUDIO_CAPACITY].sample == sample) {
            Frame f = r->frames[rd++ % KR_AUDIO_CAPACITY];
            left[i] = f.left; right[i] = f.right; revision = f.revision;
        } else missing++;
    }
    atomic_store_explicit(&r->read, rd, memory_order_release);
    if (missing) {
        atomic_fetch_add_explicit(&r->underruns, 1, memory_order_relaxed);
        atomic_fetch_add_explicit(&r->missing, missing, memory_order_relaxed);
    }
    /* Seq-cst seqlock over atomic fields: readers retry at most three times. */
    atomic_fetch_add(&r->stamp, 1);
    atomic_store(&r->sample, start); atomic_store(&r->host, t->mHostTime);
    atomic_store(&r->callback_frames, n); atomic_store(&r->revision, revision);
    atomic_store(&r->valid, true);
    atomic_fetch_add(&r->stamp, 1);
}
KRAudioClock kr_audio_clock(const KRAudioRing *r) {
    KRAudioClock c = {0};
    for (int attempt = 0; attempt < 3; attempt++) {
        uint64_t before = atomic_load(&r->stamp);
        if (before & 1) continue;
        c.sample = atomic_load(&r->sample); c.host_time = atomic_load(&r->host);
        c.callback_frames = atomic_load(&r->callback_frames); c.revision = atomic_load(&r->revision);
        c.valid = atomic_load(&r->valid);
        if (atomic_load(&r->stamp) == before) break;
        c.valid = false;
    }
    c.underruns = atomic_load(&r->underruns); c.missing_frames = atomic_load(&r->missing);
    c.timestamp_error = atomic_load(&r->timestamp_error);
    return c;
}
static bool rate(int64_t n, int64_t d) { return n > 0 && n <= 1000000000 && d > 0 && d <= 1000000000; }
int64_t kr_audio_seek_sample(int64_t frame, int64_t n, int64_t d) {
    if (frame < 0 || !rate(n,d)) return -1;
    __int128 sample = (__int128)frame * d * 48000 / n;
    return sample > INT64_MAX ? -1 : (int64_t)sample;
}
int64_t kr_audio_video_frame(int64_t sample, int64_t latency, int64_t n, int64_t d) {
    if (sample < 0 || latency < 0 || !rate(n,d)) return -1;
    if (sample <= latency) return 0;
    __int128 frame = (__int128)(sample - latency) * n / ((__int128)48000 * d);
    return frame > INT64_MAX ? -1 : (int64_t)frame;
}
int64_t kr_audio_host_samples(uint64_t ticks, uint32_t n, uint32_t d) {
    if (!n || !d) return -1;
    __int128 samples = (__int128)ticks * n * 48000 / ((__int128)d * 1000000000);
    return samples > INT64_MAX ? -1 : (int64_t)samples;
}
