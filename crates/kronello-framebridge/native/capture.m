// ScreenCaptureKit adapter for FLOW-004 (ADR-0135). A detached worker owns one
// session; the service/UI never holds these objects. Frames arrive as BGRA
// CVPixelBuffers on a serial dispatch queue, are converted to opaque RGBA8 and
// queued bounded for the caller's pull. The queue back-pressures instead of
// dropping: no recorded frame is discarded here and none is re-timed.
#import <Foundation/Foundation.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

// The typed stream-output callback used below requires the macOS 14.0 SDK.
#define KRONELLO_CAPTURE_QUEUE_DEPTH 8

typedef struct {
    int32_t kind;          // 0 = display, 1 = window, 2 = application
    uint32_t display_id;   // CGDirectDisplayID; 0 selects the main display
    uint32_t window_id;    // CGWindowID for kind 1
    char bundle_id[256];   // UTF-8 NUL-terminated for kind 2
    uint32_t width;
    uint32_t height;
    uint32_t fps_num;
    uint32_t fps_den;
} KronelloCaptureSpec;

static void set_error(char *error, size_t error_size, NSString *message) {
    if (!error || !error_size) return;
    snprintf(error, error_size, "%s", message ? message.UTF8String : "unknown capture error");
}

#if defined(__MAC_14_0)
#import <ScreenCaptureKit/ScreenCaptureKit.h>

@interface KronelloCapture : NSObject<SCStreamOutput, SCStreamDelegate>
- (instancetype)initWithWidth:(uint32_t)width height:(uint32_t)height;
- (NSError *)attach:(SCStream *)stream;
- (int)nextInto:(uint8_t *)destination capacity:(size_t)capacity
     timeoutMs:(int64_t)timeoutMs error:(NSError **)error;
- (void)stop;
@end

@implementation KronelloCapture {
    SCStream *_stream;
    dispatch_queue_t _queue;
    NSMutableArray<NSData *> *_frames;
    NSCondition *_condition;
    NSError *_failure;
    BOOL _stopped;
    uint32_t _width;
    uint32_t _height;
    size_t _frame_bytes;
}
- (instancetype)initWithWidth:(uint32_t)width height:(uint32_t)height {
    self = [super init];
    if (self) {
        _width = width;
        _height = height;
        _frame_bytes = (size_t)width * (size_t)height * 4u;
        _frames = [[NSMutableArray alloc] init];
        _condition = [[NSCondition alloc] init];
    }
    return self;
}
- (NSError *)attach:(SCStream *)stream {
    _stream = stream;
    _queue = dispatch_queue_create("dev.kronello.capture",
        dispatch_queue_attr_make_with_qos_class(DISPATCH_QUEUE_SERIAL, QOS_CLASS_USER_INITIATED, 0));
    NSError *error = nil;
    if (![_stream addStreamOutput:self type:SCStreamOutputTypeScreen sampleHandlerQueue:_queue error:&error]) {
        return error;
    }
    dispatch_semaphore_t started = dispatch_semaphore_create(0);
    [_stream startCaptureWithCompletionHandler:^(NSError *failure) {
        if (failure) {
            [_condition lock];
            _failure = failure;
            [_condition broadcast];
            [_condition unlock];
        }
        dispatch_semaphore_signal(started);
    }];
    dispatch_semaphore_wait(started, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC));
    [_condition lock];
    NSError *failure = _failure;
    [_condition unlock];
    return failure;
}
// BGRA source → opaque RGBA8 (screen content records as opaque video).
- (NSData *)convert:(CMSampleBufferRef)sample {
    CVPixelBufferRef pixel = CMSampleBufferGetImageBuffer(sample);
    if (!pixel) return nil;
    if (CVPixelBufferGetPixelFormatType(pixel) != kCVPixelFormatType_32BGRA) return nil;
    if (CVPixelBufferGetWidth(pixel) != (size_t)_width ||
        CVPixelBufferGetHeight(pixel) != (size_t)_height) return nil;
    if (CVPixelBufferLockBaseAddress(pixel, kCVPixelBufferLock_ReadOnly) != kCVReturnSuccess) return nil;
    NSMutableData *frame = [NSMutableData dataWithLength:_frame_bytes];
    const uint8_t *source = CVPixelBufferGetBaseAddress(pixel);
    const size_t stride = CVPixelBufferGetBytesPerRow(pixel);
    uint8_t *destination = frame.mutableBytes;
    for (uint32_t y = 0; y < _height; y++) {
        const uint8_t *row = source + (size_t)y * stride;
        uint8_t *out = destination + (size_t)y * _width * 4u;
        for (uint32_t x = 0; x < _width; x++) {
            out[0] = row[2];
            out[1] = row[1];
            out[2] = row[0];
            out[3] = 255;
            row += 4;
            out += 4;
        }
    }
    CVPixelBufferUnlockBaseAddress(pixel, kCVPixelBufferLock_ReadOnly);
    return frame;
}
- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)sample
        ofType:(SCStreamOutputType)type {
    if (type != SCStreamOutputTypeScreen || !sample) return;
    NSData *frame = [self convert:sample];
    [_condition lock];
    if (!frame) {
        _failure = _failure ?: [NSError errorWithDomain:@"kronello.capture" code:1
            userInfo:@{NSLocalizedDescriptionKey:
                @"captured frame is not a same-size BGRA buffer"}];
    } else {
        // Backpressure, never a drop: wait for the consumer while the session
        // stays healthy and open.
        while (_frames.count >= KRONELLO_CAPTURE_QUEUE_DEPTH && !_stopped && !_failure) {
            [_condition waitUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.25]];
        }
        if (!_stopped && !_failure) [_frames addObject:frame];
    }
    [_condition broadcast];
    [_condition unlock];
}
- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    [_condition lock];
    _failure = error;
    [_condition broadcast];
    [_condition unlock];
}
// 1 = frame copied, 0 = timeout, <0 = typed error text in `error`.
- (int)nextInto:(uint8_t *)destination capacity:(size_t)capacity
     timeoutMs:(int64_t)timeoutMs error:(NSError **)error {
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:(NSTimeInterval)timeoutMs / 1000.0];
    [_condition lock];
    while (!_frames.count && !_failure && !_stopped &&
           [deadline compare:[NSDate date]] == NSOrderedDescending) {
        [_condition waitUntilDate:deadline];
    }
    NSData *frame = nil;
    if (_frames.count) { frame = _frames.firstObject; [_frames removeObjectAtIndex:0]; }
    NSError *failure = _failure;
    BOOL stopped = _stopped;
    [_condition broadcast];
    [_condition unlock];
    if (frame) {
        if (frame.length != _frame_bytes || capacity < frame.length) {
            if (error) *error = [NSError errorWithDomain:@"kronello.capture" code:2
                userInfo:@{NSLocalizedDescriptionKey: @"capture frame size mismatch"}];
            return -1;
        }
        memcpy(destination, frame.bytes, frame.length);
        return 1;
    }
    if (failure) { if (error) *error = failure; return -3; }
    if (stopped) {
        if (error) *error = [NSError errorWithDomain:@"kronello.capture" code:3
            userInfo:@{NSLocalizedDescriptionKey: @"capture session stopped"}];
        return -2;
    }
    return 0;
}
- (void)stop {
    [_condition lock];
    _stopped = YES;
    [_condition broadcast];
    [_condition unlock];
    SCStream *stream = _stream;
    if (stream) {
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        [stream stopCaptureWithCompletionHandler:^(NSError *failure) {
            (void)failure;
            dispatch_semaphore_signal(done);
        }];
        dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 10 * NSEC_PER_SEC));
    }
}
@end

int kronello_fb_capture_open(const KronelloCaptureSpec *spec, void **session_out,
                             char *error, size_t error_size) {
    @autoreleasepool {
        *session_out = NULL;
        if (!spec || spec->width == 0 || spec->height == 0 ||
            (spec->width & 1) || (spec->height & 1) || !spec->fps_num || !spec->fps_den) {
            set_error(error, error_size, @"invalid capture spec");
            return -1;
        }
        if (![NSProcessInfo.processInfo isOperatingSystemAtLeastVersion:(NSOperatingSystemVersion){14, 0, 0}]) {
            set_error(error, error_size, @"ScreenCaptureKit requires macOS 14.0 or later");
            return -4;
        }
        dispatch_semaphore_t ready = dispatch_semaphore_create(0);
        __block SCShareableContent *content = nil;
        __block NSError *failure = nil;
        [SCShareableContent getShareableContentExcludingDesktopWindows:NO
                                                  onScreenWindowsOnly:YES
                                                completionHandler:^(SCShareableContent *c, NSError *e) {
            content = c;
            failure = e;
            dispatch_semaphore_signal(ready);
        }];
        dispatch_semaphore_wait(ready, dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC));
        if (!content) {
            set_error(error, error_size, failure ? failure.localizedDescription :
                @"screen capture permission denied or shareable content unavailable");
            return -2;
        }
        SCContentFilter *filter = nil;
        if (spec->kind == 0) {
            SCDisplay *display = nil;
            if (spec->display_id == 0) {
                display = content.displays.firstObject;
            } else {
                for (SCDisplay *candidate in content.displays) {
                    if (candidate.displayID == spec->display_id) { display = candidate; break; }
                }
            }
            if (!display) {
                set_error(error, error_size, @"requested display is not shareable content");
                return -2;
            }
            filter = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:@[]];
        } else if (spec->kind == 1) {
            SCWindow *window = nil;
            for (SCWindow *candidate in content.windows) {
                if (candidate.windowID == spec->window_id) { window = candidate; break; }
            }
            if (!window) {
                set_error(error, error_size, @"requested window is not shareable content");
                return -2;
            }
            filter = [[SCContentFilter alloc] initWithDesktopIndependentWindow:window];
        } else if (spec->kind == 2) {
            NSString *bundle = [NSString stringWithUTF8String:spec->bundle_id];
            if (!bundle) {
                set_error(error, error_size, @"application bundle identifier is not UTF-8");
                return -1;
            }
            SCRunningApplication *application = nil;
            for (SCRunningApplication *candidate in content.applications) {
                if ([candidate.bundleIdentifier isEqualToString:bundle]) { application = candidate; break; }
            }
            if (!application) {
                set_error(error, error_size, @"requested application is not shareable content");
                return -2;
            }
            SCDisplay *display = content.displays.firstObject;
            if (!display) {
                set_error(error, error_size, @"no shareable display for application capture");
                return -2;
            }
            filter = [[SCContentFilter alloc] initWithDisplay:display
                                        includingApplications:@[application]
                                             exceptingWindows:@[]];
        } else {
            set_error(error, error_size, @"unknown capture target kind");
            return -1;
        }
        if (!filter) {
            set_error(error, error_size, @"capture filter construction failed");
            return -2;
        }
        SCStreamConfiguration *config = [[SCStreamConfiguration alloc] init];
        config.width = spec->width;
        config.height = spec->height;
        config.minimumFrameInterval = CMTimeMake((int64_t)spec->fps_den, (int32_t)spec->fps_num);
        config.pixelFormat = kCVPixelFormatType_32BGRA;
        config.showsCursor = NO;
        KronelloCapture *capture =
            [[KronelloCapture alloc] initWithWidth:spec->width height:spec->height];
        SCStream *stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:capture];
        NSError *attach = [capture attach:stream];
        if (!stream || attach) {
            set_error(error, error_size, attach ? attach.localizedDescription :
                @"screen capture stream could not be created");
            return -3;
        }
        *session_out = (__bridge_retained void *)capture;
        return 0;
    }
}

int kronello_fb_capture_next(void *session, uint8_t *destination, size_t capacity,
                             int64_t timeout_ms, char *error, size_t error_size) {
    @autoreleasepool {
        if (!session) {
            set_error(error, error_size, @"closed capture session");
            return -1;
        }
        NSError *failure = nil;
        int status = [(__bridge KronelloCapture *)session nextInto:destination
                                                        capacity:capacity
                                                       timeoutMs:timeout_ms
                                                           error:&failure];
        if (status < 0) {
            set_error(error, error_size, failure ? failure.localizedDescription :
                @"capture delivery failed");
        }
        return status;
    }
}

void kronello_fb_capture_close(void *session) {
    @autoreleasepool {
        if (!session) return;
        KronelloCapture *capture = (__bridge_transfer KronelloCapture *)session;
        [capture stop];
    }
}
#else
int kronello_fb_capture_open(const KronelloCaptureSpec *spec, void **session_out,
                             char *error, size_t error_size) {
    (void)spec;
    *session_out = NULL;
    set_error(error, error_size,
        @"ScreenCaptureKit SDK headers unavailable (macOS 14.0 SDK required)");
    return -4;
}
int kronello_fb_capture_next(void *session, uint8_t *destination, size_t capacity,
                             int64_t timeout_ms, char *error, size_t error_size) {
    (void)session; (void)destination; (void)capacity; (void)timeout_ms;
    set_error(error, error_size, @"capture session unavailable");
    return -1;
}
void kronello_fb_capture_close(void *session) { (void)session; }
#endif
