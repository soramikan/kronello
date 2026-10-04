#ifndef KRONELLO_FFI_H
#define KRONELLO_FFI_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* ABI v1. Buffers are UTF-8, readable during the call, and copied by Rust.
 * Input limit: 16 MiB. Handles and request IDs are opaque integers.
 * Zero means invalid input/handle, backpressure, or thread creation failure.
 * Open is asynchronous: poll completion request_id 0 for project.info.
 * Every service request still includes its explicit project path.
 * Optional worker_executable is a same-version kronello CLI for render.submit.
 * No disk I/O, compilation, or GPU work runs on the calling thread. */
uint64_t kronello_open(const uint8_t *path, size_t len, const uint8_t *worker_executable, size_t worker_executable_len);
void kronello_close(uint64_t handle);
uint64_t kronello_call(uint64_t handle, const uint8_t *json, size_t len);
uint64_t kronello_subscribe(uint64_t handle, bool enable);
/* Poll returns {"request_id":N,"response_json":"<shared Response JSON>"} or
 * {"notification":"revision_changed|job_progress","response_json":"<shared Response JSON>"}.
 * JSON text framing preserves arbitrary-precision unknown document numbers.
 * Native control/preview completions have status plus preview metadata.
 * Nonblocking; null means empty/closed. Free each result exactly once.
 * Revision/job notifications coalesce; request completions never drop.
 * Maximum 64 accepted, unpolled requests per session. */
char *kronello_poll(uint64_t handle);
void kronello_free(char *ptr);
/* Main thread: install a live CAMetalLayer on an NSView before attach.
 * Rust retains the layer before returning. Do not change device/pixelFormat
 * after attach. Release occurs after replacement or queued work on close.
 * Close is nonblocking; queued work finishes on the worker.
 * Resize/redraw execute in FIFO order. Redraw JSON is shared render.frame;
 * region.pixels must equal the attached/resized surface dimensions.
 * Zero/minimized sizes should not be submitted. Pixel data never uses JSON.
 * Non-macOS surface operations return UNSUPPORTED_FEATURE. */
uint64_t kronello_surface_attach(uint64_t handle, void *metal_layer, uint32_t width, uint32_t height);
uint64_t kronello_surface_resize(uint64_t handle, uint32_t width, uint32_t height);
uint64_t kronello_surface_redraw(uint64_t handle, const uint8_t *json, size_t len);
#ifdef __cplusplus
}
#endif
#endif
