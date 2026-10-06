/* Instrument allocations in the actual production consumer translation unit.
 * A synthetic callback test, not host realtime playback acceptance evidence. */
#include <stdlib.h>
#include <assert.h>
#include <stdio.h>
#include <pthread.h>
#include <sched.h>
static _Thread_local int consumer_active;
static _Thread_local unsigned allocations;
static void *tracked_malloc(size_t size) { if (consumer_active) allocations++; return malloc(size); }
static void *tracked_calloc(size_t n, size_t size) { if (consumer_active) allocations++; return calloc(n, size); }
static void *tracked_realloc(void *p, size_t size) { if (consumer_active) allocations++; return realloc(p, size); }
static void tracked_free(void *p) { if (consumer_active) allocations++; free(p); }
#define malloc tracked_malloc
#define calloc tracked_calloc
#define realloc tracked_realloc
#define free tracked_free
#include "../Sources/CKronelloFFI/playback.c"
#undef malloc
#undef calloc
#undef realloc
#undef free

static void consume(KRAudioRing *ring, int64_t sample, unsigned n, float *left, float *right) {
    AudioTimeStamp t = {0}; t.mSampleTime = sample; t.mHostTime = (uint64_t)sample + 100;
    t.mFlags = kAudioTimeStampSampleTimeValid | kAudioTimeStampHostTimeValid;
    struct { UInt32 count; AudioBuffer buffers[2]; } b = {2, {{1, n*sizeof(float), left}, {1, n*sizeof(float), right}}};
    consumer_active = 1;
    kr_audio_consume(ring, &t, n, (AudioBufferList *)&b);
    consumer_active = 0; assert(allocations == 0);
}
static void clock_math(void) {
    const int64_t rates[][2] = {{24,1}, {24000,1001}, {30000,1001}};
    for (unsigned r = 0; r < 3; r++) {
        int64_t n = rates[r][0], d = rates[r][1];
        for (int64_t f = 0; f < 100000; f += 137) {
            int64_t seek = kr_audio_seek_sample(f,n,d);
            assert(seek == (__int128)f*d*48000/n);
            int64_t shown = kr_audio_video_frame(seek,0,n,d);
            assert(shown == f || shown == f-1); /* floor-grid first bucket */
            assert(kr_audio_video_frame(seek+1,0,n,d) == f);
        }
        for (int64_t hours = 1; hours <= 100000; hours *= 10) {
            int64_t samples = hours*3600*48000+12345;
            assert(kr_audio_video_frame(samples,0,n,d) == (__int128)samples*n/(48000*d));
            assert(kr_audio_video_frame(samples,1024,n,d) == (__int128)(samples-1024)*n/(48000*d));
        }
    }
    assert(kr_audio_seek_sample(INT64_MAX,1,1000000000) == -1);
    assert(kr_audio_video_frame(1,0,0,1) == -1);
    assert(kr_audio_host_samples(1000000000,1,1) == 48000);
    puts("PASS native clock: NTSC arbitrary seek, latency, 100000 hours without accumulation");
}
static void snapshot_and_seek(void) {
    KRAudioRing *r = kr_audio_create(); assert(r);
    float block[8192], left[512], right[512];
    for (int i = 0; i < 8192; i++) block[i] = 0.125f;
    assert(kr_audio_push(r,block,4096,0,7));
    for (int i = 0; i < 8192; i++) block[i] = 0.25f;
    assert(kr_audio_push(r,block,4096,4096,8));
    for (int i = 0; i < 16; i++) {
        consume(r,i*512,512,left,right);
        for (int j = 0; j < 512; j++) assert(left[j] == (i < 8 ? 0.125f : 0.25f) && right[j] == left[j]);
        assert(kr_audio_clock(r).revision == (i < 8 ? 7 : 8));
    }
    consume(r,8192,512,left,right);
    for (int i = 0; i < 512; i++) assert(left[i] == 0 && right[i] == 0);
    assert(kr_audio_clock(r).underruns == 1 && kr_audio_clock(r).missing_frames == 512);
    assert(kr_audio_push(r,block,512,8192,8)); /* late data */
    assert(kr_audio_push(r,block,512,8704,8));
    consume(r,8704,512,left,right);
    assert(left[0] == 0.25f && kr_audio_clock(r).underruns == 1);
    /* Flush at arbitrary sample: seek and stop/resume retain the same integer. */
    kr_audio_reset(r,219419);
    assert(kr_audio_push(r,block,512,219419,9));
    consume(r,4567,512,left,right); assert(kr_audio_clock(r).sample == 219419);
    kr_audio_reset(r,219500);
    assert(kr_audio_push(r,block,512,219500,9));
    consume(r,0,512,left,right); assert(kr_audio_clock(r).sample == 219500);
    consume(r,0,512,left,right); /* A backwards/non-advancing timestamp fails explicitly. */
    assert(kr_audio_clock(r).timestamp_error);
    for (int i = 0; i < 512; i++) assert(left[i] == 0 && right[i] == 0);
    kr_audio_free(r);
    puts("PASS native ring: atomic revision blocks, underrun silence/skip, seek flush, exact resume, zero consumer allocations");
}
static void *produce(void *ptr) {
    KRAudioRing *r = ptr; float block[512];
    for (int64_t s = 0; s < 1048576; s += 256) {
        for (int i = 0; i < 256; i++) { block[2*i] = (float)((s+i)%65536); block[2*i+1] = -block[2*i]; }
        while (!kr_audio_push(r,block,256,s,1)) sched_yield();
    }
    return NULL;
}
static void concurrent_spsc(void) {
    KRAudioRing *r = kr_audio_create(); assert(r); pthread_t producer;
    assert(pthread_create(&producer,NULL,produce,r) == 0);
    float left[256], right[256];
    for (int64_t s = 0; s < 1048576; s += 256) {
        while (KR_AUDIO_CAPACITY - kr_audio_available(r) < 256) sched_yield();
        consume(r,s,256,left,right);
        for (int i = 0; i < 256; i++) assert(left[i] == (float)((s+i)%65536) && right[i] == -left[i]);
    }
    pthread_join(producer,NULL); assert(kr_audio_clock(r).underruns == 0);
    kr_audio_free(r); puts("PASS native ring: concurrent SPSC million-frame wraparound");
}
int main(void) {
    /* Keep every allocation interceptor referenced; future consumer calls trap. */
    void *p = tracked_malloc(1); p = tracked_realloc(p,2); tracked_free(p);
    clock_math(); snapshot_and_seek(); concurrent_spsc(); return 0;
}
