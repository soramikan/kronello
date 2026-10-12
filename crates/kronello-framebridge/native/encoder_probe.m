// PERF-014 isolated Metal -> IOSurface -> VideoToolbox experiment.
// No production renderer/encoder selection is changed by this optional build.
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <CoreVideo/CoreVideo.h>
#import <VideoToolbox/VideoToolbox.h>
#import <CoreMedia/CoreMedia.h>
#import <mach/mach_time.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct kr_surface_result {
    double total_ms, prepare_ms, conversion_ms, encode_ms;
    uint64_t readback_bytes, surface_hash, encoded_frames;
};
struct kr_sink { FILE *file; OSStatus status; uint64_t frames; };
static int kr_write(FILE *file, const void *bytes, size_t count) {
    return fwrite(bytes, 1, count, file) == count ? 0 : -1;
}
static void kr_encoded(void *context, void *source, OSStatus status,
                       VTEncodeInfoFlags flags, CMSampleBufferRef sample) {
    (void)source; (void)flags;
    struct kr_sink *sink = context;
    if (status || !sample) { sink->status = status ? status : -1; return; }
    CMTime pts = CMSampleBufferGetPresentationTimeStamp(sample);
    if (CMTimeCompare(pts, CMTimeMake(sink->frames, 24)) != 0 ||
        CMTimeCompare(CMSampleBufferGetDuration(sample), CMTimeMake(1, 24)) != 0) {
        sink->status = -2; return;
    }
    const uint8_t start[] = {0,0,0,1};
    CMFormatDescriptionRef format = CMSampleBufferGetFormatDescription(sample);
    int length_size = 0; size_t parameter_count = 0;
    const uint8_t *parameter = NULL; size_t parameter_size = 0;
    status = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, 0,
        &parameter, &parameter_size, &parameter_count, &length_size);
    if (status || length_size != 4) { sink->status = -3; return; }
    if (sink->frames == 0) {
        for (size_t i = 0; i < parameter_count; i++) {
            if (CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, i,
                &parameter, &parameter_size, NULL, NULL) ||
                kr_write(sink->file, start, 4) || kr_write(sink->file, parameter, parameter_size)) {
                sink->status = -4; return;
            }
        }
    }
    CMBlockBufferRef block = CMSampleBufferGetDataBuffer(sample);
    size_t size = CMBlockBufferGetDataLength(block);
    // A single compressed frame is retained only until this callback returns.
    if (size > 64 * 1024 * 1024) { sink->status = -5; return; }
    uint8_t *bytes = malloc(size);
    if (!bytes || CMBlockBufferCopyDataBytes(block, 0, size, bytes)) {
        free(bytes); sink->status = -6; return;
    }
    size_t offset = 0;
    while (offset + 4 <= size) {
        uint32_t n = ((uint32_t)bytes[offset]<<24) | ((uint32_t)bytes[offset+1]<<16) |
                     ((uint32_t)bytes[offset+2]<<8) | bytes[offset+3];
        offset += 4;
        if (n > size - offset || kr_write(sink->file, start, 4) || kr_write(sink->file, bytes+offset, n)) {
            sink->status = -7; break;
        }
        offset += n;
    }
    if (offset != size) sink->status = -8;
    free(bytes);
    sink->frames++;
}
static double kr_ms(uint64_t start) {
    mach_timebase_info_data_t timebase; mach_timebase_info(&timebase);
    return (mach_absolute_time()-start) * (double)timebase.numer / timebase.denom / 1e6;
}
static const char *kr_shader =
"#include <metal_stdlib>\nusing namespace metal;\n"
"kernel void fixture(texture2d<half,access::write> dst [[texture(0)]], constant uint2 &arg [[buffer(0)]], uint2 p [[thread_position_in_grid]]) {"
"if(p.x>=dst.get_width()||p.y>=dst.get_height())return;"
"half a=((p.x/32+p.y/32+arg.x)%2)?half(0.5):half(1.0);"
"half3 rgb=half3(half((p.x+arg.x*3)%256)/half(256),half((p.y+arg.x*5)%256)/half(256),half((p.x+p.y+arg.x)%256)/half(256))*a;"
"if(arg.y && all(p==uint2(0)))rgb.x=as_type<half>(ushort(0x7e00)); dst.write(half4(rgb,a),p); }\n"
"kernel void convert(texture2d<half,access::read> src [[texture(0)]],texture2d<float,access::write> dst [[texture(1)]],"
"device const ushort *lut [[buffer(0)]], device atomic_uint *status [[buffer(1)]], uint2 p [[thread_position_in_grid]]) {"
"if(p.x>=src.get_width()||p.y>=src.get_height())return; half4 v=src.read(p); ushort4 bits=as_type<ushort4>(v);"
"ushort3 q=ushort3(lut[bits.x],lut[bits.y],lut[bits.z]);"
"if(any(q>ushort3(255))||!isfinite(float(v.w))||v.w<half(0)||v.w>half(1))atomic_store_explicit(status,1u,memory_order_relaxed);"
"dst.write(float4(float3(q)/255.0,1.0),p); }\n";

// Caller supplies the exact CPU f32 transfer/rounding table. Status codes are
// private prototype errors, never product capability or fallback selectors.
int kr_surface_probe(const char *path, uint32_t width, uint32_t height,
                     uint32_t count, int gpu_mode, int verify, int cancel_after,
                     int invalid, const uint16_t *lut, struct kr_surface_result *result) {
    @autoreleasepool {
        uint64_t begin = mach_absolute_time();
        int failure = -10;
        FILE *file = fopen(path, "wbx");
        if (!file) return -11;
        struct kr_sink sink = {file, 0, 0};
        VTCompressionSessionRef encoder = NULL;
        CVMetalTextureCacheRef cache = NULL;
        CVPixelBufferRef pixels = NULL;
        CVMetalTextureRef cv_texture = NULL;
        // ARC locals release native Metal resources after session invalidation.
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        id<MTLCommandQueue> queue = [device newCommandQueue];
        NSError *error = nil;
        id<MTLLibrary> library = [device newLibraryWithSource:@(kr_shader) options:nil error:&error];
        id<MTLComputePipelineState> fixture = [device newComputePipelineStateWithFunction:[library newFunctionWithName:@"fixture"] error:&error];
        id<MTLComputePipelineState> convert = [device newComputePipelineStateWithFunction:[library newFunctionWithName:@"convert"] error:&error];
        id<MTLBuffer> table = [device newBufferWithBytes:lut length:65536*sizeof(uint16_t) options:MTLResourceStorageModeShared];
        id<MTLBuffer> state = [device newBufferWithLength:4 options:MTLResourceStorageModeShared];
        MTLTextureDescriptor *desc = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatRGBA16Float width:width height:height mipmapped:NO];
        desc.storageMode = MTLStorageModeShared;
        desc.usage = MTLTextureUsageShaderRead | MTLTextureUsageShaderWrite;
        id<MTLTexture> source = [device newTextureWithDescriptor:desc];
        NSMutableData *cpu = gpu_mode ? nil : [NSMutableData dataWithLength:(size_t)width*height*8];
        NSDictionary *attributes = nil;
        NSDictionary *texture_attributes = nil;
        NSDictionary *properties = nil;
        NSDictionary *specification = nil;
        if (!device || !queue || !library || !fixture || !convert || !table || !state || !source || (!gpu_mode && !cpu)) goto cleanup;
        attributes = @{
            (id)kCVPixelBufferIOSurfacePropertiesKey: @{},
            (id)kCVPixelBufferMetalCompatibilityKey: @YES};
        if (CVPixelBufferCreate(NULL,width,height,kCVPixelFormatType_32BGRA,
            (__bridge CFDictionaryRef)attributes,&pixels) || !CVPixelBufferGetIOSurface(pixels)) goto cleanup;
        if (CVMetalTextureCacheCreate(NULL,NULL,device,NULL,&cache)) goto cleanup;
        texture_attributes = @{(id)kCVMetalTextureUsage: @(MTLTextureUsageShaderWrite)};
        if (CVMetalTextureCacheCreateTextureFromImage(NULL,cache,pixels,
            (__bridge CFDictionaryRef)texture_attributes,MTLPixelFormatBGRA8Unorm,width,height,0,&cv_texture)) goto cleanup;
        if (!CVMetalTextureGetTexture(cv_texture)) goto cleanup;
        specification = @{(id)kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: @YES};
        if (VTCompressionSessionCreate(NULL,width,height,kCMVideoCodecType_H264,(__bridge CFDictionaryRef)specification,
            (__bridge CFDictionaryRef)attributes,NULL,kr_encoded,&sink,&encoder)) goto cleanup;
        properties = @{
            (id)kVTCompressionPropertyKey_AllowFrameReordering: @NO,
            (id)kVTCompressionPropertyKey_RealTime: @YES,
            (id)kVTCompressionPropertyKey_AverageBitRate: @(width*height*4),
            (id)kVTCompressionPropertyKey_ExpectedFrameRate: @24,
            (id)kVTCompressionPropertyKey_ColorPrimaries: (id)kCVImageBufferColorPrimaries_ITU_R_709_2,
            (id)kVTCompressionPropertyKey_TransferFunction: (id)kCVImageBufferTransferFunction_ITU_R_709_2,
            (id)kVTCompressionPropertyKey_YCbCrMatrix: (id)kCVImageBufferYCbCrMatrix_ITU_R_709_2};
        if (VTSessionSetProperties(encoder,(__bridge CFDictionaryRef)properties) || VTCompressionSessionPrepareToEncodeFrames(encoder)) goto cleanup;
        result->prepare_ms = kr_ms(begin);
        result->surface_hash = 14695981039346656037ULL;
        for (uint32_t frame=0; frame<count; frame++) {
            @autoreleasepool {
                if (cancel_after >= 0 && frame == (uint32_t)cancel_after) { failure=-20; goto cleanup; }
                uint64_t convert_start=mach_absolute_time();
                *(uint32_t *)state.contents=0;
                id<MTLCommandBuffer> command=[queue commandBuffer];
                id<MTLComputeCommandEncoder> compute=[command computeCommandEncoder];
                uint32_t arg[2]={frame,(uint32_t)invalid};
                [compute setComputePipelineState:fixture]; [compute setTexture:source atIndex:0];
                [compute setBytes:arg length:sizeof(arg) atIndex:0];
                [compute dispatchThreads:MTLSizeMake(width,height,1) threadsPerThreadgroup:MTLSizeMake(16,16,1)];
                if (gpu_mode) {
                    [compute setComputePipelineState:convert]; [compute setTexture:source atIndex:0];
                    [compute setTexture:CVMetalTextureGetTexture(cv_texture) atIndex:1];
                    [compute setBuffer:table offset:0 atIndex:0]; [compute setBuffer:state offset:0 atIndex:1];
                    [compute dispatchThreads:MTLSizeMake(width,height,1) threadsPerThreadgroup:MTLSizeMake(16,16,1)];
                }
                [compute endEncoding]; [command commit]; [command waitUntilCompleted];
                if (command.status != MTLCommandBufferStatusCompleted || *(uint32_t *)state.contents) { failure=-21; goto cleanup; }
                if (!gpu_mode) {
                    [source getBytes:cpu.mutableBytes bytesPerRow:(size_t)width*8 fromRegion:MTLRegionMake2D(0,0,width,height) mipmapLevel:0];
                    result->readback_bytes+=(uint64_t)width*height*8;
                    if (CVPixelBufferLockBaseAddress(pixels,0)) goto cleanup;
                    uint8_t *dst=CVPixelBufferGetBaseAddress(pixels); size_t stride=CVPixelBufferGetBytesPerRow(pixels);
                    uint16_t *input=cpu.mutableBytes;
                    int bad=0;
                    for(uint32_t y=0;y<height;y++)for(uint32_t x=0;x<width;x++) {
                        uint16_t *p=input+((size_t)y*width+x)*4;
                        uint16_t r=lut[p[0]],g=lut[p[1]],b=lut[p[2]];
                        if(r>255||g>255||b>255||p[3]>0x3c00)bad=1;
                        uint8_t *q=dst+y*stride+x*4; q[0]=b;q[1]=g;q[2]=r;q[3]=255;
                    }
                    if(CVPixelBufferUnlockBaseAddress(pixels,0))goto cleanup;
                    if(bad){failure=-21;goto cleanup;}
                }
                if(verify) {
                    if(CVPixelBufferLockBaseAddress(pixels,kCVPixelBufferLock_ReadOnly))goto cleanup;
                    uint8_t *p=CVPixelBufferGetBaseAddress(pixels);size_t stride=CVPixelBufferGetBytesPerRow(pixels);
                    for(uint32_t y=0;y<height;y++)for(uint32_t x=0;x<width*4;x++)result->surface_hash=(result->surface_hash^p[y*stride+x])*1099511628211ULL;
                    if(CVPixelBufferUnlockBaseAddress(pixels,kCVPixelBufferLock_ReadOnly))goto cleanup;
                }
                result->conversion_ms+=kr_ms(convert_start);
                uint64_t encode_start=mach_absolute_time();
                if(VTCompressionSessionEncodeFrame(encoder,pixels,CMTimeMake(frame,24),CMTimeMake(1,24),NULL,NULL,NULL) ||
                   VTCompressionSessionCompleteFrames(encoder,kCMTimeInvalid) || sink.status || sink.frames!=frame+1)goto cleanup;
                // Completion consumes this input before the single surface is reused.
                result->encode_ms+=kr_ms(encode_start);
            }
        }
        failure=0;
cleanup:
        // Even error/cancel paths invalidate the session before callback context,
        // pixel buffer, cache, or Metal texture can be released.
        if(encoder){VTCompressionSessionInvalidate(encoder);CFRelease(encoder);}
        if(cv_texture)CFRelease(cv_texture);
        if(pixels)CFRelease(pixels);
        if(cache)CFRelease(cache);
        if(fclose(file))failure=-22;
        if(failure)remove(path);
        result->encoded_frames=sink.frames;
        result->total_ms=kr_ms(begin);
        return failure;
    }
}
