#ifndef KRONELLO_PLAYBACK_H
#define KRONELLO_PLAYBACK_H
#include <AudioToolbox/AudioToolbox.h>
#include <stdint.h>
#include <stdbool.h>
/* Native callback support, independent of Rust/JSON. C11 lock-free atomics.
 * One producer, one consumer. Reset/free ONLY after engine stop and producer
 * barrier. Publish whole blocks; no allocation, locks, I/O or foreign runtime
 * work in kr_audio_consume. 32768 frames = 682.67 ms maximum queued edits. */
#define KR_AUDIO_CAPACITY 32768
typedef struct KRAudioRing KRAudioRing;
typedef struct {
    int64_t sample;
    uint64_t host_time;
    uint64_t underruns;
    uint64_t missing_frames;
    uint64_t revision;
    uint32_t callback_frames;
    bool valid;
    bool timestamp_error;
} KRAudioClock;
KRAudioRing *kr_audio_create(void);
void kr_audio_free(KRAudioRing *ring);
void kr_audio_reset(KRAudioRing *ring, int64_t origin);
uint32_t kr_audio_available(const KRAudioRing *ring);
bool kr_audio_push(KRAudioRing *ring, const float *interleaved, uint32_t frames, int64_t start, uint64_t revision);
void kr_audio_consume(KRAudioRing *ring, const AudioTimeStamp *time, uint32_t frames, AudioBufferList *buffers);
KRAudioClock kr_audio_clock(const KRAudioRing *ring);
/* Exact floor math. -1 is invalid/overflow. Rates bounded to 1..1e9. */
int64_t kr_audio_seek_sample(int64_t frame, int64_t fps_num, int64_t fps_den);
int64_t kr_audio_video_frame(int64_t sample, int64_t latency, int64_t fps_num, int64_t fps_den);
int64_t kr_audio_host_samples(uint64_t elapsed_ticks, uint32_t timebase_num, uint32_t timebase_den);
#endif
