// IO-001 (ADR-0134): runtime detection of external output frameworks and the
// Syphon Metal publish boundary. Syphon and the vendor SDKs are never linked
// or bundled; every probe is a dlopen attempt plus an Objective-C class
// lookup, so absent frameworks leave the process untouched.
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <dlfcn.h>

// Syphon.framework is loaded at runtime (BSD-2-Clause; never linked), so its
// headers are unavailable here. Declare the SyphonMetalServer selectors used
// below on NSObject purely for type checking; the real class answers them.
@interface NSObject (KronelloSyphonSelectors)
- (instancetype)initWithName:(NSString *)name
                      device:(id<MTLDevice>)device
                     options:(NSDictionary *)options;
@property(readonly) BOOL hasClients;
- (void)publishFrameTexture:(id<MTLTexture>)textureToPublish
            onCommandBuffer:(id<MTLCommandBuffer>)commandBuffer
                imageRegion:(NSRect)region
                    flipped:(BOOL)flipped;
- (void)stop;
@end

static int kr_dlopen_paths(NSString *const *paths, NSUInteger count) {
    for (NSUInteger i = 0; i < count; i++) {
        const char *path = paths[i].fileSystemRepresentation;
        if (path != NULL && dlopen(path, RTLD_LAZY | RTLD_LOCAL) != NULL) {
            return 1;
        }
    }
    return 0;
}

int kr_output_syphon_detected(void) {
    static int detected = -1;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        if (NSClassFromString(@"SyphonMetalServer") != Nil) {
            detected = 1;
            return;
        }
        NSString *bundled = [NSBundle.mainBundle.privateFrameworksURL.path
            stringByAppendingPathComponent:@"Syphon.framework/Syphon"];
        NSString *paths[] = {
            bundled,
            @"/Library/Frameworks/Syphon.framework/Versions/A/Syphon",
            @"/Library/Frameworks/Syphon.framework/Syphon",
            @"Syphon.framework/Versions/A/Syphon",
        };
        if (kr_dlopen_paths(paths, 4)) {
            detected = NSClassFromString(@"SyphonMetalServer") != Nil;
        } else {
            detected = 0;
        }
    });
    return detected;
}

int kr_output_decklink_detected(void) {
    static int detected = -1;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSString *paths[] = {
            @"/Library/Frameworks/DeckLinkAPI.framework/DeckLinkAPI",
        };
        detected = kr_dlopen_paths(paths, 1);
    });
    return detected;
}

int kr_output_ndi_detected(void) {
    static int detected = -1;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSString *paths[] = {
            @"/usr/local/lib/libndi.dylib",
            @"/Library/NDI SDK for Apple/lib/macOS/libndi.dylib",
            @"libndi.dylib",
        };
        detected = kr_dlopen_paths(paths, 3);
    });
    return detected;
}

// The server pointer is a retained SyphonMetalServer, created only after
// kr_output_syphon_detected reported the framework present.
void *kr_syphon_server_create(const char *name, void *device) {
    if (device == NULL || kr_output_syphon_detected() == 0) {
        return NULL;
    }
    Class cls = NSClassFromString(@"SyphonMetalServer");
    if (cls == Nil) {
        return NULL;
    }
    NSString *serverName = name != NULL ? @(name) : @"Kronello Program";
    // initWithName:device:options: returns nil when the connection manager
    // cannot start; that failure must surface to the caller as NULL.
    id server = [[cls alloc] initWithName:serverName
                                   device:(__bridge id<MTLDevice>)device
                                  options:nil];
    if (server == nil) {
        return NULL;
    }
    return (void *)CFBridgingRetain(server);
}

int kr_syphon_has_clients(void *server) {
    if (server == NULL) {
        return 0;
    }
    return [(__bridge id)server hasClients] ? 1 : 0;
}

int kr_syphon_publish(void *server, void *texture, void *command_queue,
                      unsigned width, unsigned height) {
    if (server == NULL || texture == NULL || command_queue == NULL) {
        return 0;
    }
    id<MTLCommandBuffer> buffer =
        [(__bridge id<MTLCommandQueue>)command_queue commandBuffer];
    if (buffer == nil) {
        return 0;
    }
    [(__bridge id)server publishFrameTexture:(__bridge id<MTLTexture>)texture
                    onCommandBuffer:buffer
                        imageRegion:NSMakeRect(0, 0, width, height)
                            flipped:NO];
    [buffer commit];
    // The publish work is a bounded blit into the server's IOSurface; waiting
    // for completion is this output's frame pacing and keeps the source
    // texture's lifetime unambiguous for the caller.
    [buffer waitUntilCompleted];
    return buffer.status == MTLCommandBufferStatusCompleted ? 1 : 0;
}

void kr_syphon_server_stop(void *server) {
    if (server != NULL) {
        [(__bridge id)server stop];
    }
}

void kr_syphon_server_release(void *server) {
    if (server != NULL) {
        CFBridgingRelease(server);
    }
}
