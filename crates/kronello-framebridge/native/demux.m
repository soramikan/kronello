// Compressed local-file demux only. VideoToolbox decoding remains in Rust.
#import <AVFoundation/AVFoundation.h>
#import <Foundation/Foundation.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

// Validate the compressed description itself, independent of caller metadata.
static int resident_format_supported(CMFormatDescriptionRef format) {
    // CoreMedia may omit primaries/transfer extensions even when the original
    // MOV sample entry contains an explicit nclc/nclx color atom. Read that
    // authoritative compressed description without decoding pixels.
    int tagged709=0;
    CFTypeRef entry=CMFormatDescriptionGetExtension(format,kCMFormatDescriptionExtension_VerbatimSampleDescription);
    if (entry && CFGetTypeID(entry)==CFDataGetTypeID()) {
        const UInt8 *b=CFDataGetBytePtr(entry); CFIndex n=CFDataGetLength(entry);
        for (CFIndex offset=86; offset+8<=n;) {
            uint32_t length=((uint32_t)b[offset]<<24)|((uint32_t)b[offset+1]<<16)|((uint32_t)b[offset+2]<<8)|b[offset+3];
            if (length<8 || length>(uint64_t)(n-offset)) break;
            if (length>=18 && memcmp(b+offset+4,"colr",4)==0 &&
                (memcmp(b+offset+8,"nclc",4)==0 || memcmp(b+offset+8,"nclx",4)==0)) {
                tagged709=b[offset+12]==0 && b[offset+13]==1 && b[offset+14]==0 &&
                    b[offset+15]==1 && b[offset+16]==0 && b[offset+17]==1;
                if (memcmp(b+offset+8,"nclx",4)==0 && (length<19 || (b[offset+18]&128))) return 0;
            }
            offset+=length;
        }
    }
    const CFStringRef keys[] = {kCMFormatDescriptionExtension_ColorPrimaries,
        kCMFormatDescriptionExtension_TransferFunction, kCMFormatDescriptionExtension_YCbCrMatrix};
    const CFStringRef expected[] = {kCVImageBufferColorPrimaries_ITU_R_709_2,
        kCVImageBufferTransferFunction_ITU_R_709_2, kCVImageBufferYCbCrMatrix_ITU_R_709_2};
    for (size_t i=0; i<3; i++) {
        CFTypeRef value=CMFormatDescriptionGetExtension(format,keys[i]);
        if (!value) { if (tagged709) continue; return 0; }
        if (CFGetTypeID(value)!=CFStringGetTypeID()) return 0;
        if (!CFEqual(value,expected[i]) && !(i==1 && tagged709 && CFEqual(value,kCVImageBufferTransferFunction_sRGB))) return 0;
    }
    CFTypeRef range=CMFormatDescriptionGetExtension(format,kCMFormatDescriptionExtension_FullRangeVideo);
    if (range && (CFGetTypeID(range)!=CFBooleanGetTypeID() || CFBooleanGetValue(range))) return 0;
    CFTypeRef atoms=CMFormatDescriptionGetExtension(format,kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms);
    if (!atoms || CFGetTypeID(atoms)!=CFDictionaryGetTypeID()) return 0;
    FourCharCode codec=CMFormatDescriptionGetMediaSubType(format);
    CFStringRef key=codec==kCMVideoCodecType_H264?CFSTR("avcC"):codec==kCMVideoCodecType_HEVC?CFSTR("hvcC"):NULL;
    if (!key) return 0;
    CFTypeRef data=CFDictionaryGetValue((CFDictionaryRef)atoms,key);
    if (!data || CFGetTypeID(data)!=CFDataGetTypeID()) return 0;
    const UInt8 *bytes=CFDataGetBytePtr((CFDataRef)data);
    CFIndex size=CFDataGetLength((CFDataRef)data);
    if (codec==kCMVideoCodecType_HEVC) {
        // ISO/IEC 14496-15 HEVCDecoderConfigurationRecord: chroma_format,
        // bitDepthLumaMinus8 and bitDepthChromaMinus8 are authoritative.
        return size>=23 && bytes[0]==1 && (bytes[16]&3)==1 && (bytes[17]&7)==0 && (bytes[18]&7)==0;
    }
    // Baseline/Main/High AVC profiles admit 8-bit 4:2:0. High10/422/444 and
    // incomplete records are unsupported, even if caller says yuv420p.
    if (size<7 || bytes[0]!=1) return 0;
    if (bytes[1]==66 || bytes[1]==77) return 1;
    if (bytes[1]!=100) return 0;
    // High-profile avcC carries explicit chroma/bit-depth extension fields.
    // Profile alone is insufficient to prove non-monochrome 4:2:0.
    CFIndex offset=6;
    unsigned sps=bytes[5]&31;
    for (unsigned i=0;i<sps;i++) {
        if (offset+2>size) return 0;
        unsigned length=((unsigned)bytes[offset]<<8)|bytes[offset+1];offset+=2;
        if (offset+length>size) return 0;offset+=length;
    }
    if (offset>=size) return 0;
    unsigned pps=bytes[offset++];
    for (unsigned i=0;i<pps;i++) {
        if (offset+2>size) return 0;
        unsigned length=((unsigned)bytes[offset]<<8)|bytes[offset+1];offset+=2;
        if (offset+length>size) return 0;offset+=length;
    }
    return offset+4<=size && (bytes[offset]&3)==1 && (bytes[offset+1]&7)==0 && (bytes[offset+2]&7)==0;
}

// Return retained compressed samples in a bounded caller buffer. The caller
// releases every sample even on error; no decompressed CPU pixels are requested.
int kronello_fb_read_samples(const char *path, uint32_t stream, int64_t time_num, int64_t time_den, int canonical_origin,
                            void **samples, size_t capacity, size_t *count,
                            char *error, size_t error_size) {
    @autoreleasepool {
        *count = 0;
        NSURL *url = [NSURL fileURLWithPath:[NSString stringWithUTF8String:path]];
        AVURLAsset *asset = [AVURLAsset URLAssetWithURL:url options:nil];
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        NSArray<AVAssetTrack *> *tracks = asset.tracks;
#pragma clang diagnostic pop
        if (stream >= tracks.count || ![tracks[stream].mediaType isEqualToString:AVMediaTypeVideo]) {
            snprintf(error, error_size, "AVFoundation track index does not select video");
            return -1;
        }
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        CMTimeRange range = tracks[stream].timeRange;
#pragma clang diagnostic pop
        CMTime end = CMTimeRangeGetEnd(range);
        if (!canonical_origin && (time_den <= 0 || range.start.timescale <= 0 || end.timescale <= 0 ||
            (__int128)time_num * range.start.timescale < (__int128)range.start.value * time_den ||
            (__int128)time_num * end.timescale >= (__int128)end.value * time_den)) {
            snprintf(error, error_size, "FRAME_NOT_FOUND: requested time outside half-open video track interval");
            return -6;
        }
        NSError *failure = nil;
        AVAssetReader *reader = [[AVAssetReader alloc] initWithAsset:asset error:&failure];
        AVAssetReaderTrackOutput *output = [[AVAssetReaderTrackOutput alloc] initWithTrack:tracks[stream] outputSettings:nil];
        output.alwaysCopiesSampleData = NO;
        if (!reader || ![reader canAddOutput:output]) {
            snprintf(error, error_size, "compressed asset reader unavailable");
            return -2;
        }
        [reader addOutput:output];
        if (![reader startReading]) {
            snprintf(error, error_size, "%s", reader.error.localizedDescription.UTF8String);
            return -3;
        }
        size_t compressed_bytes = 0;
        CMSampleBufferRef sample;
        while ((sample = [output copyNextSampleBuffer])) {
            // AVAssetReader may emit zero-sample edit/discontinuity markers.
            // They have no compressed format and are not decoder input frames.
            if (CMSampleBufferGetNumSamples(sample) == 0) {
                CFRelease(sample);
                continue;
            }
            if (!CMSampleBufferGetFormatDescription(sample) || !CMSampleBufferGetDataBuffer(sample)) {
                CFRelease(sample);
                [reader cancelReading];
                snprintf(error, error_size, "nonempty compressed sample lacks format/data buffer");
                return -7;
            }
            if (!resident_format_supported(CMSampleBufferGetFormatDescription(sample))) {
                CFRelease(sample);
                [reader cancelReading];
                snprintf(error,error_size,"actual compressed color/range/chroma/bit depth unsupported or unverified; require tagged SDR BT709 8-bit 420");
                return -8;
            }
            compressed_bytes += CMSampleBufferGetTotalSampleSize(sample);
            if (*count == capacity || compressed_bytes > 128 * 1024 * 1024) {
                CFRelease(sample);
                [reader cancelReading];
                snprintf(error, error_size, "compressed sample budget exceeded");
                return -4;
            }
            samples[(*count)++] = (void *)sample;
        }
        if (reader.status != AVAssetReaderStatusCompleted) {
            snprintf(error, error_size, "%s", reader.error.localizedDescription.UTF8String);
            return -5;
        }
        return 0;
    }
}
