/* Audited ABI boundary. FFmpeg resources never leave this file. All libav
 * calls use symbols from explicitly opened shared libraries, never static FFmpeg.
 * Each context owns its libraries and decoder/encoder children. No global state. */
#include <libavformat/avformat.h>
#include <libavcodec/avcodec.h>
#include <libavutil/imgutils.h>
#include <libavutil/pixdesc.h>
#include <libavutil/hwcontext.h>
#include <libswscale/swscale.h>
#include <libswresample/swresample.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <dlfcn.h>
#endif
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>

#define STR_(x) #x
#define STR(x) STR_(x)
typedef struct Km {
    void *libs[5];
    char error[512];
    int error_code;
    char error_operation[256];
    char error_detail[256];
    __typeof__(&avutil_version) avutil_version;
    __typeof__(&avutil_license) avutil_license;
    __typeof__(&avutil_configuration) avutil_configuration;
    __typeof__(&av_version_info) av_version_info;
    __typeof__(&av_frame_alloc) av_frame_alloc;
    __typeof__(&av_frame_free) av_frame_free;
    __typeof__(&av_frame_unref) av_frame_unref;
    __typeof__(&av_frame_get_buffer) av_frame_get_buffer;
    __typeof__(&av_frame_make_writable) av_frame_make_writable;
    __typeof__(&av_image_get_buffer_size) av_image_get_buffer_size;
    __typeof__(&av_image_copy_to_buffer) av_image_copy_to_buffer;
    __typeof__(&av_get_pix_fmt_name) av_get_pix_fmt_name;
    __typeof__(&av_get_pix_fmt) av_get_pix_fmt;
    __typeof__(&av_image_fill_arrays) av_image_fill_arrays;
    __typeof__(&av_color_primaries_name) av_color_primaries_name;
    __typeof__(&av_color_transfer_name) av_color_transfer_name;
    __typeof__(&av_color_space_name) av_color_space_name;
    __typeof__(&av_color_range_name) av_color_range_name;
    __typeof__(&av_hwdevice_iterate_types) av_hwdevice_iterate_types;
    __typeof__(&av_hwdevice_get_type_name) av_hwdevice_get_type_name;
    __typeof__(&av_dict_set) av_dict_set;
    __typeof__(&av_dict_free) av_dict_free;
    __typeof__(&av_strerror) av_strerror;
    __typeof__(&avcodec_version) avcodec_version;
    __typeof__(&avcodec_license) avcodec_license;
    __typeof__(&avcodec_configuration) avcodec_configuration;
    __typeof__(&av_codec_iterate) av_codec_iterate;
    __typeof__(&av_codec_is_encoder) av_codec_is_encoder;
    __typeof__(&av_codec_is_decoder) av_codec_is_decoder;
    __typeof__(&avcodec_find_decoder) avcodec_find_decoder;
    __typeof__(&avcodec_find_decoder_by_name) avcodec_find_decoder_by_name;
    __typeof__(&avcodec_find_encoder_by_name) avcodec_find_encoder_by_name;
    __typeof__(&avcodec_alloc_context3) avcodec_alloc_context3;
    __typeof__(&avcodec_parameters_to_context) avcodec_parameters_to_context;
    __typeof__(&avcodec_parameters_from_context) avcodec_parameters_from_context;
    __typeof__(&avcodec_open2) avcodec_open2;
    __typeof__(&avcodec_free_context) avcodec_free_context;
    __typeof__(&avcodec_flush_buffers) avcodec_flush_buffers;
    __typeof__(&avcodec_send_packet) avcodec_send_packet;
    __typeof__(&avcodec_receive_frame) avcodec_receive_frame;
    __typeof__(&avcodec_send_frame) avcodec_send_frame;
    __typeof__(&avcodec_receive_packet) avcodec_receive_packet;
    __typeof__(&av_packet_alloc) av_packet_alloc;
    __typeof__(&av_packet_free) av_packet_free;
    __typeof__(&av_packet_unref) av_packet_unref;
    __typeof__(&av_packet_rescale_ts) av_packet_rescale_ts;
    __typeof__(&avformat_version) avformat_version;
    __typeof__(&avformat_license) avformat_license;
    __typeof__(&avformat_configuration) avformat_configuration;
    __typeof__(&avformat_alloc_context) avformat_alloc_context;
    __typeof__(&avformat_open_input) avformat_open_input;
    __typeof__(&avformat_find_stream_info) avformat_find_stream_info;
    __typeof__(&av_find_best_stream) av_find_best_stream;
    __typeof__(&av_read_frame) av_read_frame;
    __typeof__(&avformat_seek_file) avformat_seek_file;
    __typeof__(&avformat_close_input) avformat_close_input;
    __typeof__(&avformat_alloc_output_context2) avformat_alloc_output_context2;
    __typeof__(&avformat_new_stream) avformat_new_stream;
    __typeof__(&avio_open) avio_open;
    __typeof__(&avio_closep) avio_closep;
    __typeof__(&avformat_write_header) avformat_write_header;
    __typeof__(&av_interleaved_write_frame) av_interleaved_write_frame;
    __typeof__(&av_write_trailer) av_write_trailer;
    __typeof__(&avformat_free_context) avformat_free_context;
    __typeof__(&av_mallocz) av_mallocz;
    __typeof__(&av_realloc_array) av_realloc_array;
    __typeof__(&av_free) av_free;
    __typeof__(&swscale_version) swscale_version;
    __typeof__(&swscale_license) swscale_license;
    __typeof__(&swscale_configuration) swscale_configuration;
    __typeof__(&sws_getContext) sws_getContext;
    __typeof__(&sws_getCoefficients) sws_getCoefficients;
    __typeof__(&sws_setColorspaceDetails) sws_setColorspaceDetails;
    __typeof__(&sws_scale) sws_scale;
    __typeof__(&sws_freeContext) sws_freeContext;
    __typeof__(&av_channel_layout_copy) av_channel_layout_copy;
    __typeof__(&av_channel_layout_uninit) av_channel_layout_uninit;
    __typeof__(&av_channel_layout_default) av_channel_layout_default;
    __typeof__(&av_rescale_rnd) av_rescale_rnd;
    __typeof__(&av_compare_ts) av_compare_ts;
    __typeof__(&av_dict_get) av_dict_get;
    __typeof__(&avcodec_parameters_copy) avcodec_parameters_copy;
    __typeof__(&avcodec_get_name) avcodec_get_name;
    __typeof__(&swresample_version) swresample_version;
    __typeof__(&swresample_license) swresample_license;
    __typeof__(&swresample_configuration) swresample_configuration;
    __typeof__(&swr_alloc_set_opts2) swr_alloc_set_opts2;
    __typeof__(&swr_init) swr_init;
    __typeof__(&swr_convert) swr_convert;
    __typeof__(&swr_get_delay) swr_get_delay;
    __typeof__(&swr_free) swr_free;
} Km;
static int fail(Km *k, int code, const char *where) {
    char detail[256] = {0};
    if (k->av_strerror) k->av_strerror(code, detail, sizeof(detail));
    k->error_code=code;
    snprintf(k->error_operation, sizeof(k->error_operation), "%s", where);
    snprintf(k->error_detail, sizeof(k->error_detail), "%s", detail);
    snprintf(k->error, sizeof(k->error), "%s: %s (%d)", where, detail, code);
    return code < 0 ? code : -1;
}
void km_close(Km *k) {
    if (!k) return;
    for (int i=4; i>=0; --i) if (k->libs[i]) {
#ifdef _WIN32
        FreeLibrary((HMODULE)k->libs[i]);
#else
        dlclose(k->libs[i]);
#endif
    }
    free(k);
}
const char *km_error(Km *k) { return k->error; }
int km_error_code(Km *k) { return k->error_code; }
const char *km_error_operation(Km *k) { return k->error_operation; }
const char *km_error_detail(Km *k) { return k->error_detail; }
Km *km_open(const char *directory, char *error, size_t capacity) {
    Km *k = calloc(1, sizeof(*k));
    if (!k) { snprintf(error, capacity, "allocation failed"); return NULL; }
    const char *names[5] = {"avutil", "avcodec", "avformat", "swscale", "swresample"};
    int majors[5] = {LIBAVUTIL_VERSION_MAJOR, LIBAVCODEC_VERSION_MAJOR, LIBAVFORMAT_VERSION_MAJOR, LIBSWSCALE_VERSION_MAJOR, LIBSWRESAMPLE_VERSION_MAJOR};
    for (int i=0; i<5; ++i) {
        char path[4096];
#ifdef _WIN32
        int n=snprintf(path, sizeof(path), "%s\\%s-%d.dll", directory, names[i], majors[i]);
#elif defined(__APPLE__)
        int n=snprintf(path, sizeof(path), "%s/lib%s.%d.dylib", directory, names[i], majors[i]);
#else
        int n=snprintf(path, sizeof(path), "%s/lib%s.so.%d", directory, names[i], majors[i]);
#endif
        if (n<0 || (size_t)n>=sizeof(path)) { snprintf(error, capacity, "library path too long"); km_close(k); return NULL; }
#ifdef _WIN32
        /* Canonical Windows paths use the extended \\?\ namespace, which does
         * not normalize forward slashes. Preserve that prefix and normalize
         * separators before the Unicode loader; never widen DLL search. */
        for (int j=0;j<n;++j) if (path[j]=='/') path[j]='\\';
        wchar_t wide[4096];
        if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, wide, 4096)) {
            snprintf(error, capacity, "invalid UTF-8 library path"); km_close(k); return NULL;
        }
        k->libs[i]=(void *)LoadLibraryExW(wide, NULL,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        if (!k->libs[i]) { snprintf(error, capacity, "%s: Windows loader error %lu", path, (unsigned long)GetLastError()); km_close(k); return NULL; }
#else
        k->libs[i]=dlopen(path, RTLD_NOW | RTLD_LOCAL);
        if (!k->libs[i]) { snprintf(error, capacity, "%s: %s", path, dlerror()); km_close(k); return NULL; }
#endif
    }
#ifdef _WIN32
#define KM_SYMBOL(handle, name) ((void *)GetProcAddress((HMODULE)(handle), (name)))
#else
#define KM_SYMBOL(handle, name) dlsym((handle), (name))
#endif
#define LOAD(i, name) do { *(void **)(&k->name) = KM_SYMBOL(k->libs[i], #name); if (!k->name) { snprintf(error, capacity, "missing symbol: %s", #name); km_close(k); return NULL; } } while(0)
    LOAD(0, avutil_version);
    LOAD(0, avutil_license);
    LOAD(0, avutil_configuration);
    LOAD(0, av_version_info);
    LOAD(0, av_frame_alloc);
    LOAD(0, av_frame_free);
    LOAD(0, av_frame_unref);
    LOAD(0, av_frame_get_buffer);
    LOAD(0, av_frame_make_writable);
    LOAD(0, av_image_get_buffer_size);
    LOAD(0, av_image_copy_to_buffer);
    LOAD(0, av_get_pix_fmt_name);
    LOAD(0, av_get_pix_fmt);
    LOAD(0, av_image_fill_arrays);
    LOAD(0, av_color_primaries_name);
    LOAD(0, av_color_transfer_name);
    LOAD(0, av_color_space_name);
    LOAD(0, av_color_range_name);
    LOAD(0, av_hwdevice_iterate_types);
    LOAD(0, av_hwdevice_get_type_name);
    LOAD(0, av_dict_set);
    LOAD(0, av_dict_free);
    LOAD(0, av_strerror);
    LOAD(1, avcodec_version);
    LOAD(1, avcodec_license);
    LOAD(1, avcodec_configuration);
    LOAD(1, av_codec_iterate);
    LOAD(1, av_codec_is_encoder);
    LOAD(1, av_codec_is_decoder);
    LOAD(1, avcodec_find_decoder);
    LOAD(1, avcodec_find_decoder_by_name);
    LOAD(1, avcodec_find_encoder_by_name);
    LOAD(1, avcodec_alloc_context3);
    LOAD(1, avcodec_parameters_to_context);
    LOAD(1, avcodec_parameters_from_context);
    LOAD(1, avcodec_open2);
    LOAD(1, avcodec_free_context);
    LOAD(1, avcodec_flush_buffers);
    LOAD(1, avcodec_send_packet);
    LOAD(1, avcodec_receive_frame);
    LOAD(1, avcodec_send_frame);
    LOAD(1, avcodec_receive_packet);
    LOAD(1, av_packet_alloc);
    LOAD(1, av_packet_free);
    LOAD(1, av_packet_unref);
    LOAD(1, av_packet_rescale_ts);
    LOAD(2, avformat_version);
    LOAD(2, avformat_license);
    LOAD(2, avformat_configuration);
    LOAD(2, avformat_alloc_context);
    LOAD(2, avformat_open_input);
    LOAD(2, avformat_find_stream_info);
    LOAD(2, av_find_best_stream);
    LOAD(2, av_read_frame);
    LOAD(2, avformat_seek_file);
    LOAD(2, avformat_close_input);
    LOAD(2, avformat_alloc_output_context2);
    LOAD(2, avformat_new_stream);
    LOAD(2, avio_open);
    LOAD(2, avio_closep);
    LOAD(2, avformat_write_header);
    LOAD(2, av_interleaved_write_frame);
    LOAD(2, av_write_trailer);
    LOAD(2, avformat_free_context);
    LOAD(0, av_mallocz);
    LOAD(0, av_realloc_array);
    LOAD(0, av_free);
    LOAD(3, swscale_version);
    LOAD(3, swscale_license);
    LOAD(3, swscale_configuration);
    LOAD(3, sws_getContext);
    LOAD(3, sws_getCoefficients);
    LOAD(3, sws_setColorspaceDetails);
    LOAD(3, sws_scale);
    LOAD(3, sws_freeContext);
    LOAD(0, av_channel_layout_copy);
    LOAD(0, av_channel_layout_uninit);
    LOAD(0, av_channel_layout_default);
    LOAD(0, av_rescale_rnd);
    LOAD(0, av_compare_ts);
    LOAD(0, av_dict_get);
    LOAD(1, avcodec_parameters_copy);
    LOAD(1, avcodec_get_name);
    LOAD(4, swresample_version);
    LOAD(4, swresample_license);
    LOAD(4, swresample_configuration);
    LOAD(4, swr_alloc_set_opts2);
    LOAD(4, swr_init);
    LOAD(4, swr_convert);
    LOAD(4, swr_get_delay);
    LOAD(4, swr_free);
#undef LOAD
    unsigned versions[5] = {k->avutil_version(), k->avcodec_version(), k->avformat_version(), k->swscale_version(), k->swresample_version()};
    for (int i=0;i<5;++i) if ((versions[i]>>16)!=(unsigned)majors[i]) { snprintf(error, capacity, "FFmpeg ABI major mismatch"); km_close(k); return NULL; }
    return k;
}
const char *km_info(Km *k, int index) {
    switch(index) {
        case 0:return k->av_version_info();
        case 1:return k->avutil_license();
        case 2:return k->avutil_configuration();
        case 3:return k->avcodec_license();
        case 4:return k->avcodec_configuration();
        case 5:return k->avformat_license();
        case 6:return k->avformat_configuration();
        case 7:return k->swscale_license();
        case 8:return k->swscale_configuration();
        case 9:return k->swresample_license();
        case 10:return k->swresample_configuration();
        default:return "";
    }
}
unsigned km_version(Km *k, int index) {
    switch(index) {case 0:return k->avutil_version(); case 1:return k->avcodec_version(); case 2:return k->avformat_version(); case 3:return k->swscale_version(); default:return k->swresample_version();}
}
int km_codec_hardware_capable(int capabilities) {
    /* VideoToolbox is HYBRID because its wrapper can allow software. The
     * public encoder path explicitly sets allow_sw=0 before opening it. */
    return !!(capabilities & (AV_CODEC_CAP_HARDWARE | AV_CODEC_CAP_HYBRID));
}
const char *km_codec(Km *k, void **cursor, int *encoder, int *decoder, int *hardware) {
    const AVCodec *c=k->av_codec_iterate(cursor);
    if (!c) return NULL;
    *encoder=k->av_codec_is_encoder(c); *decoder=k->av_codec_is_decoder(c);
    *hardware=km_codec_hardware_capable(c->capabilities);
    return c->name;
}
const char *km_hw(Km *k, int *kind) {
    *kind=k->av_hwdevice_iterate_types(*kind);
    return *kind==AV_HWDEVICE_TYPE_NONE ? NULL : k->av_hwdevice_get_type_name(*kind);
}

typedef struct Decoder {
    Km *k;
    AVFormatContext *format;
    AVCodecContext *codec;
    AVPacket *packet;
    AVFrame *frame;
    int stream, draining;
} Decoder;
void km_decoder_close(Decoder *d) {
    if (!d) return;
    Km *k=d->k;
    k->av_frame_free(&d->frame); k->av_packet_free(&d->packet);
    k->avcodec_free_context(&d->codec); k->avformat_close_input(&d->format); free(d);
}
static Decoder *decoder_open(Km *k, const char *path, enum AVMediaType kind, int stream) {
    Decoder *d=calloc(1,sizeof(*d));
    if(!d) { fail(k,AVERROR(ENOMEM),"decoder allocation"); return NULL; }
    d->k=k; d->format=k->avformat_alloc_context();
    if(!d->format) goto alloc_failed;
    /* Only local single-file demuxers. Playlists, URL protocols and demuxers
     * that fetch external resources are outside the public media API. */
    AVDictionary *options=NULL;
    k->av_dict_set(&options,"protocol_whitelist","file",0);
    k->av_dict_set(&options,"format_whitelist","nut,matroska,webm,mov,avi,mpegts,mpeg,ogg,wav,png_pipe,jpeg_pipe,mxf,mp3,flac,gif",0);
    int ret=k->avformat_open_input(&d->format,path,NULL,&options);
    k->av_dict_free(&options);
    if(ret<0) { fail(k,ret,"open input"); goto failed; }
    ret=k->avformat_find_stream_info(d->format,NULL);
    if(ret<0) { fail(k,ret,"stream info"); goto failed; }
    const AVCodec *codec=NULL;
    d->stream=k->av_find_best_stream(d->format,kind,stream,-1,&codec,0);
    if(d->stream<0) { fail(k,d->stream,"requested stream"); goto failed; }
    if(d->format->streams[d->stream]->codecpar->codec_id==AV_CODEC_ID_AV1) {
        codec=k->avcodec_find_decoder_by_name("libdav1d");
        if(!codec)codec=k->avcodec_find_decoder_by_name("libaom-av1");
        if(!codec){fail(k,AVERROR_DECODER_NOT_FOUND,"AV1 software decoder unavailable");goto failed;}
    }
    if(codec->capabilities & AV_CODEC_CAP_HARDWARE) { fail(k,AVERROR(ENOSYS),"explicit software decoder unavailable"); goto failed; }
    d->codec=k->avcodec_alloc_context3(codec);
    d->packet=k->av_packet_alloc(); d->frame=k->av_frame_alloc();
    if(!d->codec || !d->packet || !d->frame) goto alloc_failed;
    ret=k->avcodec_parameters_to_context(d->codec,d->format->streams[d->stream]->codecpar);
    if(ret<0) { fail(k,ret,"codec parameters"); goto failed; }
    d->codec->thread_count=1;
    ret=k->avcodec_open2(d->codec,codec,NULL);
    if(ret<0) { fail(k,ret,"open decoder"); goto failed; }
    return d;
alloc_failed:fail(k,AVERROR(ENOMEM),"decoder allocation");
failed:km_decoder_close(d);return NULL;
}
Decoder *km_decoder_open(Km *k, const char *path) { return decoder_open(k,path,AVMEDIA_TYPE_VIDEO,-1); }
Decoder *km_decoder_open_stream(Km *k, const char *path, int stream) { return decoder_open(k,path,AVMEDIA_TYPE_VIDEO,stream); }
const char *km_decoder_name(Decoder *d) { return d->codec->codec->name; }
void km_decoder_time_base(Decoder *d, int *num, int *den) { AVRational t=d->format->streams[d->stream]->time_base; *num=t.num; *den=t.den; }
int64_t km_decoder_origin(Decoder *d) { int64_t start=d->format->streams[d->stream]->start_time; return start==AV_NOPTS_VALUE ? 0 : start; }
int km_decoder_stream(Decoder *d) { return d->stream; }
int64_t km_decoder_duration(Decoder *d) { return d->format->streams[d->stream]->duration; }
int km_decoder_seek(Decoder *d, int64_t target) {
    int ret=d->k->avformat_seek_file(d->format,d->stream,INT64_MIN,target,target,AVSEEK_FLAG_BACKWARD);
    if(ret<0)return fail(d->k,ret,"seek");
    d->k->avcodec_flush_buffers(d->codec); d->k->av_packet_unref(d->packet); d->k->av_frame_unref(d->frame); d->draining=0; return 0;
}
int km_decoder_next(Decoder *d) {
    Km *k=d->k; k->av_frame_unref(d->frame);
    for (;;) {
        int ret=k->avcodec_receive_frame(d->codec,d->frame);
        if(ret==0)return 1;
        if(ret==AVERROR_EOF)return 0;
        if(ret!=AVERROR(EAGAIN))return fail(k,ret,"receive frame");
        if(d->draining)return fail(k,AVERROR_INVALIDDATA,"drain stalled");
        for (;;) { ret=k->av_read_frame(d->format,d->packet); if(ret<0 || d->packet->stream_index==d->stream)break; k->av_packet_unref(d->packet); }
        if(ret==AVERROR_EOF) { d->draining=1; ret=k->avcodec_send_packet(d->codec,NULL); }
        else if(ret<0)return fail(k,ret,"read packet");
        else { ret=k->avcodec_send_packet(d->codec,d->packet); k->av_packet_unref(d->packet); }
        if(ret<0)return fail(k,ret,"send packet");
    }
}
/* Plain layout, mirrored by repr(C); packed pixels are copied into Rust-owned bytes. */
typedef struct FrameInfo { int64_t pts,duration; int width,height,format,primaries,transfer,matrix,range; } FrameInfo;
void km_frame_info(Decoder *d, FrameInfo *out) {
    AVFrame *f=d->frame;
    *out=(FrameInfo){f->best_effort_timestamp,f->duration,f->width,f->height,f->format,f->color_primaries,f->color_trc,f->colorspace,f->color_range};
}
const char *km_frame_label(Decoder *d, int index) {
    Km *k=d->k;AVFrame *f=d->frame;
    switch(index) {case 0:return k->av_get_pix_fmt_name(f->format); case 1:return k->av_color_primaries_name(f->color_primaries); case 2:return k->av_color_transfer_name(f->color_trc);case 3:return k->av_color_space_name(f->colorspace);default:return k->av_color_range_name(f->color_range);}
}
int km_frame_copy(Decoder *d, uint8_t *buffer, int size) {
    AVFrame *f=d->frame;
    if(!buffer)return d->k->av_image_get_buffer_size(f->format,f->width,f->height,1);
    return d->k->av_image_copy_to_buffer(buffer,size,(const uint8_t *const *)f->data,f->linesize,f->format,f->width,f->height,1);
}
/* Explicit SDR BT.709 matrix/range conversion. Rust validates source tags and
 * inverse-transfers RGB; this shim does not tone-map or guess color semantics. */
int km_video_rgba(Km *k, const uint8_t *input, int input_size, const char *format,
                  int width, int height, int full_range, uint8_t *output) {
    enum AVPixelFormat fmt=k->av_get_pix_fmt(format);
    int size=k->av_image_get_buffer_size(fmt,width,height,1);
    if(size<0 || size!=input_size)return fail(k,AVERROR(EINVAL),"video pixel layout");
    uint8_t *planes[4]={0}; int strides[4]={0};
    int ret=k->av_image_fill_arrays(planes,strides,input,fmt,width,height,1);
    if(ret<0)return fail(k,ret,"video planes");
    struct SwsContext *sws=k->sws_getContext(width,height,fmt,width,height,AV_PIX_FMT_RGBA,SWS_BILINEAR,NULL,NULL,NULL);
    if(!sws)return fail(k,AVERROR(EINVAL),"video RGBA converter");
    const int *coeff=k->sws_getCoefficients(SWS_CS_ITU709);
    ret=k->sws_setColorspaceDetails(sws,coeff,full_range,coeff,1,0,1<<16,1<<16);
    if(ret>=0) { uint8_t *dst[4]={output,NULL,NULL,NULL};int dst_stride[4]={width*4,0,0,0};ret=k->sws_scale(sws,(const uint8_t *const *)planes,strides,0,height,dst,dst_stride); }
    k->sws_freeContext(sws);
    return ret==height ? 0 : fail(k,ret<0?ret:AVERROR(EINVAL),"video RGBA conversion");
}

/* Preserve source precision before Rust applies the pinned transfer function. */
int km_video_rgba64(Km *k, const uint8_t *input, int input_size, const char *format,
                   int width, int height, int full_range, int bt2020, uint8_t *output) {
    enum AVPixelFormat fmt=k->av_get_pix_fmt(format);
    int size=k->av_image_get_buffer_size(fmt,width,height,1);
    if(size<0 || size!=input_size)return fail(k,AVERROR(EINVAL),"HDR video pixel layout");
    uint8_t *planes[4]={0}; int strides[4]={0};
    int ret=k->av_image_fill_arrays(planes,strides,input,fmt,width,height,1);
    if(ret<0)return fail(k,ret,"HDR video planes");
    struct SwsContext *sws=k->sws_getContext(width,height,fmt,width,height,AV_PIX_FMT_RGBA64LE,SWS_BILINEAR,NULL,NULL,NULL);
    if(!sws)return fail(k,AVERROR(EINVAL),"HDR RGBA64 converter");
    const int *coeff=k->sws_getCoefficients(bt2020?SWS_CS_BT2020:SWS_CS_ITU709);
    ret=k->sws_setColorspaceDetails(sws,coeff,full_range,coeff,1,0,1<<16,1<<16);
    if(ret>=0) { uint8_t *dst[4]={output,NULL,NULL,NULL};int dst_stride[4]={width*8,0,0,0};ret=k->sws_scale(sws,(const uint8_t *const *)planes,strides,0,height,dst,dst_stride); }
    k->sws_freeContext(sws);
    return ret==height ? 0 : fail(k,ret<0?ret:AVERROR(EINVAL),"HDR RGBA64 conversion");
}

typedef struct Encoder {
    Km *k; AVFormatContext *format; AVCodecContext *codec; AVFrame *frame; AVPacket *packet;
    struct SwsContext *sws; AVStream *stream; int header, input_stride; int64_t duration;
} Encoder;
void km_encoder_close(Encoder *e) {
    if(!e)return;
    Km *k=e->k;
    if(e->sws)k->sws_freeContext(e->sws);
    k->av_frame_free(&e->frame); k->av_packet_free(&e->packet); k->avcodec_free_context(&e->codec);
    if(e->format) { if(e->format->pb)k->avio_closep(&e->format->pb); k->avformat_free_context(e->format); }
    free(e);
}
Encoder *km_encoder_open_color(Km *k, const char *path, const char *name, int width, int height, int num, int den, int hdr) {
    Encoder *e=calloc(1,sizeof(*e)); if(!e) {fail(k,AVERROR(ENOMEM),"encoder allocation");return NULL;} e->k=k;
    const AVCodec *codec=k->avcodec_find_encoder_by_name(name);
    if(!codec) {fail(k,AVERROR_ENCODER_NOT_FOUND,"encoder unavailable");goto failed;}
    /* Closed intermediate shapes: ProRes 422p10 MOV and delivery MP4 yuv420p.
     * DNx delivery profiles go through km_encoder_open_dnx, which carries the
     * versioned profile/pixel-format table explicitly. */
    int prores=!strcmp(name,"prores_ks");
    if(hdr && (!prores || (hdr!=1 && hdr!=2))){fail(k,AVERROR(EINVAL),"unsupported HDR encoder/profile");goto failed;}
    e->input_stride=width*(hdr?8:4);
    int ret=k->avformat_alloc_output_context2(&e->format,NULL,prores?"mov":"mp4",path);
    if(ret<0 || !e->format) {fail(k,ret<0?ret:AVERROR(ENOMEM),"output context");goto failed;}
    e->codec=k->avcodec_alloc_context3(codec); e->frame=k->av_frame_alloc();e->packet=k->av_packet_alloc();e->stream=k->avformat_new_stream(e->format,NULL);
    if(!e->codec || !e->frame || !e->packet || !e->stream){fail(k,AVERROR(ENOMEM),"encoder allocation");goto failed;}
    e->codec->width=width; e->codec->height=height;e->codec->time_base=(AVRational){num,den};e->codec->framerate=(AVRational){den,num};
    e->codec->pix_fmt=prores?AV_PIX_FMT_YUV422P10LE:AV_PIX_FMT_YUV420P;
    e->codec->color_primaries=AVCOL_PRI_BT709;e->codec->color_trc=AVCOL_TRC_BT709;e->codec->colorspace=AVCOL_SPC_BT709;e->codec->color_range=AVCOL_RANGE_MPEG;
    if(hdr){e->codec->color_primaries=AVCOL_PRI_BT2020;e->codec->color_trc=hdr==1?AVCOL_TRC_SMPTE2084:AVCOL_TRC_ARIB_STD_B67;e->codec->colorspace=AVCOL_SPC_BT2020_NCL;}
    e->codec->thread_count=1;e->codec->bit_rate=2000000;
    if(e->format->oformat->flags & AVFMT_GLOBALHEADER)e->codec->flags|=AV_CODEC_FLAG_GLOBAL_HEADER;
    AVDictionary *options=NULL;
    if(hdr){k->av_dict_set(&options,"profile","3",0);}
    if(strstr(name,"videotoolbox")){k->av_dict_set(&options,"allow_sw","0",0);}
    if(!strcmp(name,"libsvtav1")){k->av_dict_set(&options,"preset","12",0);k->av_dict_set(&options,"svtav1-params","lp=1",0);}
    if(!strcmp(name,"libaom-av1")){k->av_dict_set(&options,"cpu-used","8",0);k->av_dict_set(&options,"usage","realtime",0);}
    ret=k->avcodec_open2(e->codec,codec,&options);k->av_dict_free(&options);
    if(ret<0){
        char where[256];
        snprintf(where,sizeof(where),"avcodec_open2(%s, %s, %dx%d, time_base=%d/%d)",name,k->av_get_pix_fmt_name(e->codec->pix_fmt),width,height,num,den);
        fail(k,ret,where);goto failed;
    }
    e->stream->time_base=e->codec->time_base;
    ret=k->avcodec_parameters_from_context(e->stream->codecpar,e->codec);
    if(ret<0){fail(k,ret,"encoder parameters");goto failed;}
    ret=k->avio_open(&e->format->pb,path,AVIO_FLAG_WRITE);if(ret<0){fail(k,ret,"open output");goto failed;}
    AVDictionary *mux_options=NULL;
    char timescale[32];snprintf(timescale,sizeof(timescale),"%d",den);
    k->av_dict_set(&mux_options,"video_track_timescale",timescale,0);
    ret=k->avformat_write_header(e->format,&mux_options);k->av_dict_free(&mux_options);if(ret<0){fail(k,ret,"write header");goto failed;}e->header=1;
    e->frame->format=e->codec->pix_fmt;e->frame->width=width;e->frame->height=height;
    e->frame->color_primaries=e->codec->color_primaries;e->frame->color_trc=e->codec->color_trc;e->frame->colorspace=e->codec->colorspace;e->frame->color_range=e->codec->color_range;
    ret=k->av_frame_get_buffer(e->frame,32);if(ret<0){fail(k,ret,"frame buffer");goto failed;}
    e->sws=k->sws_getContext(width,height,hdr?AV_PIX_FMT_RGBA64LE:AV_PIX_FMT_RGBA,width,height,e->codec->pix_fmt,SWS_BILINEAR,NULL,NULL,NULL);
    if(!e->sws){fail(k,AVERROR(ENOMEM),"pixel conversion");goto failed;}
    const int *coeff=k->sws_getCoefficients(hdr?SWS_CS_BT2020:SWS_CS_ITU709);
    ret=k->sws_setColorspaceDetails(e->sws,coeff,1,coeff,0,0,1<<16,1<<16);
    if(ret<0){fail(k,ret,"BT.709 pixel conversion");goto failed;}
    return e;
failed:km_encoder_close(e);return NULL;
}
Encoder *km_encoder_open(Km *k,const char *path,const char *name,int width,int height,int num,int den) {
    return km_encoder_open_color(k,path,name,width,height,num,den,0);
}
/* MEDIA-004 DNxHD/DNxHR delivery. The closed table pins the FFmpeg encoder
 * profile and intermediate pixel format per versioned profile; DNxHD family
 * legality (resolution/frame-rate/bitrate) is enforced by the encoder itself
 * so non-compliant requests fail open with a typed FFmpeg error. */
static const struct { const char *profile; enum AVPixelFormat fmt; } DNX_KINDS[6]={
    {"dnxhd",AV_PIX_FMT_YUV422P},
    {"dnxhr_lb",AV_PIX_FMT_YUV422P},
    {"dnxhr_sq",AV_PIX_FMT_YUV422P},
    {"dnxhr_hq",AV_PIX_FMT_YUV422P},
    {"dnxhr_hqx",AV_PIX_FMT_YUV422P10LE},
    {"dnxhr_444",AV_PIX_FMT_YUV444P10LE},
};
Encoder *km_encoder_open_dnx(Km *k,const char *path,int width,int height,int num,int den,int kind) {
    if(kind<0 || kind>5){fail(k,AVERROR(EINVAL),"unknown DNx profile");return NULL;}
    if(width<=0 || width&1 || height<=0 || height&1){fail(k,AVERROR(EINVAL),"DNx requires positive even dimensions");return NULL;}
    Encoder *e=calloc(1,sizeof(*e)); if(!e){fail(k,AVERROR(ENOMEM),"encoder allocation");return NULL;} e->k=k;
    const AVCodec *codec=k->avcodec_find_encoder_by_name("dnxhd");
    if(!codec){fail(k,AVERROR_ENCODER_NOT_FOUND,"encoder unavailable");goto failed;}
    e->input_stride=width*4;
    int ret=k->avformat_alloc_output_context2(&e->format,NULL,"mov",path);
    if(ret<0 || !e->format){fail(k,ret<0?ret:AVERROR(ENOMEM),"output context");goto failed;}
    e->codec=k->avcodec_alloc_context3(codec);e->frame=k->av_frame_alloc();e->packet=k->av_packet_alloc();e->stream=k->avformat_new_stream(e->format,NULL);
    if(!e->codec || !e->frame || !e->packet || !e->stream){fail(k,AVERROR(ENOMEM),"encoder allocation");goto failed;}
    e->codec->width=width;e->codec->height=height;e->codec->time_base=(AVRational){num,den};e->codec->framerate=(AVRational){den,num};
    e->codec->pix_fmt=DNX_KINDS[kind].fmt;
    e->codec->color_primaries=AVCOL_PRI_BT709;e->codec->color_trc=AVCOL_TRC_BT709;e->codec->colorspace=AVCOL_SPC_BT709;e->codec->color_range=AVCOL_RANGE_MPEG;
    e->codec->thread_count=1;
    AVDictionary *options=NULL;
    k->av_dict_set(&options,"profile",DNX_KINDS[kind].profile,0);
    ret=k->avcodec_open2(e->codec,codec,&options);k->av_dict_free(&options);
    if(ret<0){
        char where[256];
        snprintf(where,sizeof(where),"avcodec_open2(dnxhd, %s, %dx%d, time_base=%d/%d)",DNX_KINDS[kind].profile,width,height,num,den);
        fail(k,ret,where);goto failed;
    }
    e->stream->time_base=e->codec->time_base;
    ret=k->avcodec_parameters_from_context(e->stream->codecpar,e->codec);if(ret<0){fail(k,ret,"encoder parameters");goto failed;}
    ret=k->avio_open(&e->format->pb,path,AVIO_FLAG_WRITE);if(ret<0){fail(k,ret,"open output");goto failed;}
    AVDictionary *mux_options=NULL;
    char timescale[32];snprintf(timescale,sizeof(timescale),"%d",den);
    k->av_dict_set(&mux_options,"video_track_timescale",timescale,0);
    ret=k->avformat_write_header(e->format,&mux_options);k->av_dict_free(&mux_options);if(ret<0){fail(k,ret,"write header");goto failed;}e->header=1;
    e->frame->format=e->codec->pix_fmt;e->frame->width=width;e->frame->height=height;
    e->frame->color_primaries=e->codec->color_primaries;e->frame->color_trc=e->codec->color_trc;e->frame->colorspace=e->codec->colorspace;e->frame->color_range=e->codec->color_range;
    ret=k->av_frame_get_buffer(e->frame,32);if(ret<0){fail(k,ret,"frame buffer");goto failed;}
    e->sws=k->sws_getContext(width,height,AV_PIX_FMT_RGBA,width,height,e->codec->pix_fmt,SWS_BILINEAR,NULL,NULL,NULL);
    if(!e->sws){fail(k,AVERROR(ENOMEM),"pixel conversion");goto failed;}
    const int *coeff=k->sws_getCoefficients(SWS_CS_ITU709);
    ret=k->sws_setColorspaceDetails(e->sws,coeff,1,coeff,0,0,1<<16,1<<16);
    if(ret<0){fail(k,ret,"BT.709 pixel conversion");goto failed;}
    return e;
failed:km_encoder_close(e);return NULL;
}
static int write_packets(Encoder *e) {
    for(;;){
        int ret=e->k->avcodec_receive_packet(e->codec,e->packet);
        if(ret==AVERROR(EAGAIN)||ret==AVERROR_EOF)return 0;
        if(ret<0)return fail(e->k,ret,"receive packet");
        /* Input contract: callers report each submitted frame's span in
         * time_base ticks; packets delayed by the encoder reuse the last
         * submitted span, which is exact for the low-delay codecs used here. */
        e->packet->duration=e->duration;
        e->k->av_packet_rescale_ts(e->packet,e->codec->time_base,e->stream->time_base);e->packet->stream_index=e->stream->index;
        ret=e->k->av_interleaved_write_frame(e->format,e->packet);e->k->av_packet_unref(e->packet);
        if(ret<0)return fail(e->k,ret,"write packet");
    }
}
int km_encoder_frame(Encoder *e,const uint8_t *rgba,int64_t pts,int64_t duration) {
    if(duration<=0)return fail(e->k,AVERROR(EINVAL),"non-positive frame duration");
    int ret=e->k->av_frame_make_writable(e->frame);if(ret<0)return fail(e->k,ret,"writable frame");
    const uint8_t *data[4]={rgba,NULL,NULL,NULL};int stride[4]={e->input_stride,0,0,0};
    ret=e->k->sws_scale(e->sws,data,stride,0,e->codec->height,e->frame->data,e->frame->linesize);
    if(ret!=e->codec->height)return fail(e->k,AVERROR_INVALIDDATA,"pixel conversion");
    e->frame->pts=pts; e->frame->duration=duration; e->duration=duration;
    ret=e->k->avcodec_send_frame(e->codec,e->frame);if(ret<0)return fail(e->k,ret,"send frame");return write_packets(e);
}
int km_encoder_finish(Encoder *e) {
    int ret=e->k->avcodec_send_frame(e->codec,NULL);if(ret<0)return fail(e->k,ret,"flush encoder");
    ret=write_packets(e);if(ret<0)return ret;
    ret=e->k->av_write_trailer(e->format);if(ret<0)return fail(e->k,ret,"write trailer");
    ret=e->k->avio_closep(&e->format->pb);if(ret<0)return fail(e->k,ret,"close output");return 0;
}
const char *km_encoder_format(Encoder *e){return e->k->av_get_pix_fmt_name(e->codec->pix_fmt);}

int km_encoder_frame_size(Encoder *e){return e->k->av_image_get_buffer_size(e->codec->pix_fmt,e->codec->width,e->codec->height,1);}

/* The closed ADR-0124 speaker-layout set shared with kronello-model. Any
 * other native mask or a custom/non-native order is a typed rejection. */
static int audio_mask_supported(uint64_t mask) {
    return mask==AV_CH_LAYOUT_MONO || mask==AV_CH_LAYOUT_STEREO ||
        mask==AV_CH_LAYOUT_5POINT1 || mask==AV_CH_LAYOUT_5POINT1_BACK ||
        mask==AV_CH_LAYOUT_7POINT1;
}
static int audio_mask_channels(uint64_t mask) {
    int n=0;while(mask){mask&=mask-1;++n;}return n;
}
/* Audio decoder owns one swresample context. The source's own channel layout
 * is preserved through resampling; conversion is the caller's explicit choice. */
typedef struct AudioDecoder {
    Decoder *d; SwrContext *swr; float *samples; int count, rate, channels, format;
    int64_t mask;
    int initialized, eof; int64_t pts; int input_samples;
} AudioDecoder;
void km_audio_close(AudioDecoder *a) {
    if(!a)return;
    if(a->swr)a->d->k->swr_free(&a->swr);
    free(a->samples);km_decoder_close(a->d);free(a);
}
AudioDecoder *km_audio_open(Km *k,const char *path,int stream) {
    AudioDecoder *a=calloc(1,sizeof(*a));
    if(!a){fail(k,AVERROR(ENOMEM),"audio allocation");return NULL;}
    a->d=decoder_open(k,path,AVMEDIA_TYPE_AUDIO,stream);
    if(!a->d){free(a);return NULL;}
    return a;
}
void km_audio_time_base(AudioDecoder *a,int *num,int *den) { km_decoder_time_base(a->d,num,den); }
int km_audio_rate(AudioDecoder *a) { return a->rate; }
int km_audio_channels(AudioDecoder *a) { return a->channels; }
int64_t km_audio_mask(AudioDecoder *a) { return a->mask; }
int64_t km_audio_pts(AudioDecoder *a) { return a->pts; }
int km_audio_input_samples(AudioDecoder *a) { return a->input_samples; }
int km_audio_count(AudioDecoder *a) { return a->count; }
int km_audio_copy(AudioDecoder *a,float *out,int capacity) {
    if(a->channels<=0 || capacity!=a->count*a->channels)return -1;
    if(capacity)memcpy(out,a->samples,(size_t)capacity*sizeof(float));
    return capacity;
}
int km_audio_next(AudioDecoder *a) {
    Km *k=a->d->k;
    if(a->eof)return 0;
    int ret=km_decoder_next(a->d);
    if(ret<0)return ret;
    AVFrame *f=a->d->frame;
    a->input_samples=ret?f->nb_samples:0;
    a->pts=ret?f->best_effort_timestamp:AV_NOPTS_VALUE;
    if(ret) {
        int channels=f->ch_layout.nb_channels;
        if(f->sample_rate<=0 || f->sample_rate>768000 || f->nb_samples<=0 || f->nb_samples>1048576)
            return fail(k,AVERROR_INVALIDDATA,"invalid audio rate or frame size");
        /* ADR-0124: only mono/stereo containers may omit the speaker mask;
         * FFmpeg's documented default for those counts is exact. Any wider
         * layout without a native mask is rejected, never guessed. */
        if(channels>=1 && channels<=2 && f->ch_layout.order==AV_CHANNEL_ORDER_UNSPEC) {
            k->av_channel_layout_uninit(&f->ch_layout);
            k->av_channel_layout_default(&f->ch_layout,channels);
        }
        if(f->ch_layout.order!=AV_CHANNEL_ORDER_NATIVE ||
            !audio_mask_supported(f->ch_layout.u.mask) ||
            channels!=audio_mask_channels(f->ch_layout.u.mask))
            return fail(k,AVERROR(ENOSYS),"unsupported audio channel layout");
        if(!a->initialized) {
            a->rate=f->sample_rate;a->channels=channels;a->format=f->format;
            a->mask=(int64_t)f->ch_layout.u.mask;
            /* Resample into the source's own layout: no fold-down, no
             * channel synthesis. Layout conversion is the caller's choice. */
            ret=k->swr_alloc_set_opts2(&a->swr,&f->ch_layout,AV_SAMPLE_FMT_FLT,48000,
                &f->ch_layout,f->format,f->sample_rate,0,NULL);
            if(ret<0)return fail(k,ret,"allocate resampler");
            ret=k->swr_init(a->swr);if(ret<0)return fail(k,ret,"initialize resampler");
            a->initialized=1;
        } else if(a->rate!=f->sample_rate || a->channels!=channels || a->format!=f->format ||
            a->mask!=(int64_t)f->ch_layout.u.mask) {
            return fail(k,AVERROR(ENOSYS),"audio format changes within stream");
        }
    } else if(!a->initialized) { a->eof=1;return 0; }
    int64_t capacity=k->av_rescale_rnd(k->swr_get_delay(a->swr,a->rate)+a->input_samples,48000,a->rate,AV_ROUND_UP);
    if(capacity<0 || capacity>2097152)return fail(k,AVERROR_INVALIDDATA,"resampler buffer budget");
    if(capacity==0){a->eof=1;return 0;}
    float *samples=realloc(a->samples,(size_t)capacity*a->channels*sizeof(float));
    if(!samples)return fail(k,AVERROR(ENOMEM),"resampler output allocation");
    a->samples=samples;
    uint8_t *out=(uint8_t *)samples;
    int count=k->swr_convert(a->swr,&out,(int)capacity,
        a->input_samples?(const uint8_t **)f->extended_data:NULL,a->input_samples);
    if(count<0)return fail(k,count,"convert audio");
    a->count=count;
    if(!a->input_samples && !count){a->eof=1;return 0;}
    return 1;
}

/* A streaming audio encoder retains one frame and one packet. Input chunks
 * must fill the declared codec block except for the final partial block. */
typedef struct AudioEncoder {
    Km *k; AVFormatContext *format; AVCodecContext *codec;
    AVFrame *frame; AVPacket *packet; AVStream *stream;
    int block, kind, partial, channels; int64_t mask, count;
} AudioEncoder;
/* Closed delivery audio kinds: PCM24 MOV, ALAC MP4, AAC-LC MP4, Opus WebM,
 * and the MEDIA-004 standalone deliverables MP3 (libmp3lame CBR 256k,
 * mono/stereo) and FLAC (native, lossless up to the closed layouts). */
static const struct {
    const char *encoder,*container;enum AVSampleFormat fmt;int64_t bitrate;
} AUDIO_KINDS[6]={
    {"pcm_s24le","mov",AV_SAMPLE_FMT_S32,0},
    {"alac","mp4",AV_SAMPLE_FMT_S32P,0},
    {"aac","mp4",AV_SAMPLE_FMT_FLTP,192000},
    {"libopus","webm",AV_SAMPLE_FMT_FLT,128000},
    {"libmp3lame","mp3",AV_SAMPLE_FMT_S32P,256000},
    {"flac","flac",AV_SAMPLE_FMT_S32,0},
};
void km_audio_encoder_close(AudioEncoder *e) {
    if(!e)return;
    Km *k=e->k;
    k->av_packet_free(&e->packet);k->av_frame_free(&e->frame);k->avcodec_free_context(&e->codec);
    if(e->format){if(e->format->pb)k->avio_closep(&e->format->pb);k->avformat_free_context(e->format);}
    free(e);
}
AudioEncoder *km_audio_encoder_open(Km *k,const char *path,int kind,
    int channels,int64_t mask) {
    AudioEncoder *e=calloc(1,sizeof(*e));
    if(!e){fail(k,AVERROR(ENOMEM),"audio allocation");return NULL;}
    if(kind<0 || kind>5){fail(k,AVERROR(EINVAL),"unknown audio kind");free(e);return NULL;}
    if(mask<0 || !audio_mask_supported((uint64_t)mask) ||
        channels!=audio_mask_channels((uint64_t)mask) || (kind==4 && channels>2)) {
        fail(k,AVERROR(ENOSYS),"unsupported audio channel layout");free(e);return NULL;
    }
    e->k=k;e->kind=kind;e->channels=channels;e->mask=mask;
    const AVCodec *encoder=k->avcodec_find_encoder_by_name(AUDIO_KINDS[kind].encoder);
    int ret=AVERROR_ENCODER_NOT_FOUND;
    if(!encoder){fail(k,ret,"audio encoder unavailable");goto failed;}
    ret=k->avformat_alloc_output_context2(&e->format,NULL,AUDIO_KINDS[kind].container,path);
    if(ret<0 || !e->format){fail(k,ret<0?ret:AVERROR(ENOMEM),"audio output context");goto failed;}
    e->codec=k->avcodec_alloc_context3(encoder);e->frame=k->av_frame_alloc();e->packet=k->av_packet_alloc();
    e->stream=k->avformat_new_stream(e->format,NULL);
    if(!e->codec || !e->frame || !e->packet || !e->stream){fail(k,AVERROR(ENOMEM),"audio allocation");goto failed;}
    e->codec->sample_rate=48000;e->codec->sample_fmt=AUDIO_KINDS[kind].fmt;
    e->codec->time_base=(AVRational){1,48000};
    if(kind==1)e->codec->bits_per_raw_sample=24;
    if(AUDIO_KINDS[kind].bitrate)e->codec->bit_rate=AUDIO_KINDS[kind].bitrate;
    e->codec->ch_layout.order=AV_CHANNEL_ORDER_NATIVE;
    e->codec->ch_layout.nb_channels=channels;
    e->codec->ch_layout.u.mask=(uint64_t)mask;
    if(e->format->oformat->flags & AVFMT_GLOBALHEADER)e->codec->flags|=AV_CODEC_FLAG_GLOBAL_HEADER;
    AVDictionary *codec_options=NULL;
    if(kind==3) {
        k->av_dict_set(&codec_options,"application","audio",0);
        k->av_dict_set(&codec_options,"frame_duration","20",0);
        k->av_dict_set(&codec_options,"vbr","on",0);
    }
    ret=k->avcodec_open2(e->codec,encoder,&codec_options);k->av_dict_free(&codec_options);
    if(ret<0){fail(k,ret,"open audio encoder");goto failed;}
    if(kind==1 && (e->codec->initial_padding!=0 || e->codec->frame_size<=0 ||
        !(encoder->capabilities & AV_CODEC_CAP_SMALL_LAST_FRAME))) {
        fail(k,AVERROR_INVALIDDATA,"ALAC requires zero priming and exact partial final frame");goto failed;
    }
    /* AAC/Opus carry signalled priming. MP3 stores the encoder delay in the
     * mp3 muxer's Xing header, so only exact frame blocking and a partial
     * final frame are required at this boundary. FLAC is lossless. */
    if((kind==2 || kind==3) && (e->codec->initial_padding<=0 || e->codec->frame_size<=0 ||
        !(encoder->capabilities & AV_CODEC_CAP_SMALL_LAST_FRAME))) {
        fail(k,AVERROR_INVALIDDATA,"lossy audio requires signalled priming and partial final frame");goto failed;
    }
    if(kind==4 && (e->codec->frame_size<=0 ||
        !(encoder->capabilities & AV_CODEC_CAP_SMALL_LAST_FRAME))) {
        fail(k,AVERROR_INVALIDDATA,"MP3 requires codec frame blocking and partial final frame");goto failed;
    }
    e->block=kind?(e->codec->frame_size>0?e->codec->frame_size:4096):4096;
    if(e->block<=0 || e->block>65536){fail(k,AVERROR_INVALIDDATA,"audio block budget");goto failed;}
    e->stream->time_base=e->codec->time_base;
    ret=k->avcodec_parameters_from_context(e->stream->codecpar,e->codec);if(ret<0){fail(k,ret,"audio parameters");goto failed;}
    ret=k->avio_open(&e->format->pb,path,AVIO_FLAG_WRITE);if(ret<0){fail(k,ret,"open audio output");goto failed;}
    AVDictionary *options=NULL;
    if(kind==1 || kind==2)k->av_dict_set(&options,"movie_timescale","48000",0);
    ret=k->avformat_write_header(e->format,&options);k->av_dict_free(&options);if(ret<0){fail(k,ret,"audio header");goto failed;}
    e->frame->format=e->codec->sample_fmt;e->frame->sample_rate=48000;e->frame->nb_samples=e->block;
    ret=k->av_channel_layout_copy(&e->frame->ch_layout,&e->codec->ch_layout);if(ret<0){fail(k,ret,"audio channel layout");goto failed;}
    ret=k->av_frame_get_buffer(e->frame,0);if(ret<0){fail(k,ret,"audio frame buffer");goto failed;}
    return e;
failed:km_audio_encoder_close(e);return NULL;
}
int km_audio_encoder_block(AudioEncoder *e){return e->block;}
static int audio_packets(AudioEncoder *e,int drain) {
    Km *k=e->k;
    for(;;) {
        int ret=k->avcodec_receive_packet(e->codec,e->packet);
        if(ret==AVERROR_EOF && drain)return 0;
        if(ret==AVERROR(EAGAIN) && !drain)return 0;
        if(ret<0)return fail(k,ret,"receive audio packet");
        k->av_packet_rescale_ts(e->packet,e->codec->time_base,e->stream->time_base);e->packet->stream_index=e->stream->index;
        ret=k->av_interleaved_write_frame(e->format,e->packet);k->av_packet_unref(e->packet);
        if(ret<0)return fail(k,ret,"write audio packet");
    }
}
int km_audio_encoder_frame(AudioEncoder *e,const int32_t *samples,int count) {
    Km *k=e->k;
    if(count<=0 || count>e->block || e->partial || e->count>INT64_MAX-count)
        return fail(k,AVERROR(EINVAL),"audio input block/order");
    int ret=k->av_frame_make_writable(e->frame);if(ret<0)return fail(k,ret,"audio writable buffer");
    e->frame->nb_samples=count;e->frame->pts=e->count;
    enum AVSampleFormat fmt=e->codec->sample_fmt;
    int channels=e->channels;
    if(fmt==AV_SAMPLE_FMT_S32)memcpy(e->frame->data[0],samples,(size_t)count*channels*sizeof(int32_t));
    else if(fmt==AV_SAMPLE_FMT_FLT) {
        float *out=(float *)e->frame->data[0];
        for(int j=0;j<count*channels;++j)out[j]=(float)(samples[j]/2147483648.0);
    } else {
        for(int ch=0;ch<channels;++ch) {
            if(fmt==AV_SAMPLE_FMT_S32P) {
                int32_t *plane=(int32_t *)e->frame->data[ch];
                for(int j=0;j<count;++j)plane[j]=samples[j*channels+ch];
            } else {
                float *plane=(float *)e->frame->data[ch];
                for(int j=0;j<count;++j)plane[j]=(float)(samples[j*channels+ch]/2147483648.0);
            }
        }
    }
    ret=k->avcodec_send_frame(e->codec,e->frame);if(ret<0)return fail(k,ret,"send audio frame");
    e->count+=count;e->partial=count<e->block;return audio_packets(e,0);
}
int km_audio_encoder_finish(AudioEncoder *e) {
    Km *k=e->k;
    /* Opus discard padding only applies when the stream spans more than one
     * packet; inputs at or below the priming margin cannot round-trip their
     * exact length and are refused rather than padded silently. */
    if(e->kind==3 && e->count<=e->block-e->codec->initial_padding)
        return fail(k,AVERROR_INVALIDDATA,"Opus input shorter than codec priming margin");
    int ret=k->avcodec_send_frame(e->codec,NULL);if(ret<0)return fail(k,ret,"flush audio encoder");
    ret=audio_packets(e,1);if(ret<0)return ret;
    ret=k->av_write_trailer(e->format);if(ret<0)return fail(k,ret,"audio trailer");
    ret=k->avio_closep(&e->format->pb);return ret<0?fail(k,ret,"close audio output"):0;
}
int km_audio_encode(Km *k,const char *path,const int32_t *samples,int64_t count,
    int kind,int channels,int64_t mask) {
    AudioEncoder *e=km_audio_encoder_open(k,path,kind,channels,mask);if(!e)return -1;
    int ret=0;
    for(int64_t i=0;i<count;i+=e->block) {
        int n=(int)((count-i)<e->block?(count-i):e->block);
        ret=km_audio_encoder_frame(e,samples+i*channels,n);if(ret<0)break;
    }
    if(ret>=0)ret=km_audio_encoder_finish(e);
    km_audio_encoder_close(e);return ret;
}

/* File-only probe and mux share the same bounded demuxer/protocol policy. */
AVFormatContext *km_probe_open(Km *k,const char *path) {
    AVFormatContext *format=NULL;AVDictionary *options=NULL;
    k->av_dict_set(&options,"protocol_whitelist","file",0);
    k->av_dict_set(&options,"format_whitelist","nut,matroska,webm,mov,avi,mpegts,mpeg,ogg,wav,mxf,mp3,flac,gif",0);
    int ret=k->avformat_open_input(&format,path,NULL,&options);k->av_dict_free(&options);
    if(ret<0){fail(k,ret,"probe open");goto failed;}
    ret=k->avformat_find_stream_info(format,NULL);if(ret<0){fail(k,ret,"probe streams");goto failed;}
    return format;
failed:k->avformat_close_input(&format);return NULL;
}
void km_probe_close(Km *k,AVFormatContext *format) { k->avformat_close_input(&format); }
int km_probe_count(AVFormatContext *format) { return (int)format->nb_streams; }
int64_t km_probe_format_duration(AVFormatContext *format) { return format->duration; }
typedef struct StreamInfo {
    int64_t start,duration,channel_mask;int kind,num,den,rate,channels,width,height;
} StreamInfo;
void km_probe_stream(AVFormatContext *format,int i,StreamInfo *out) {
    AVStream *s=format->streams[i];AVCodecParameters *p=s->codecpar;
    *out=(StreamInfo){s->start_time,s->duration,
        p->ch_layout.order==AV_CHANNEL_ORDER_NATIVE?(int64_t)p->ch_layout.u.mask:-1,
        p->codec_type,s->time_base.num,s->time_base.den,
        p->sample_rate,p->ch_layout.nb_channels,p->width,p->height};
}
const char *km_probe_codec(Km *k,AVFormatContext *format,int i) { return k->avcodec_get_name(format->streams[i]->codecpar->codec_id); }
const char *km_probe_color(Km *k,AVFormatContext *format,int i,int field) {
    AVCodecParameters *p=format->streams[i]->codecpar;
    switch(field){case 0:return k->av_get_pix_fmt_name(p->format);case 1:return k->av_color_primaries_name(p->color_primaries);case 2:return k->av_color_transfer_name(p->color_trc);case 3:return k->av_color_space_name(p->color_space);default:return k->av_color_range_name(p->color_range);}
}
uint32_t km_probe_codec_tag(AVFormatContext *format,int i) { return format->streams[i]->codecpar->codec_tag; }
const char *km_probe_tag(Km *k,AVFormatContext *format,const char *key) {
    const AVDictionaryEntry *entry=k->av_dict_get(format->metadata,key,NULL,0);
    return entry?entry->value:"";
}
/* MEDIA-004 chapters: count plus per-entry id/start/end/time_base/title. */
int km_probe_chapter_count(AVFormatContext *format) { return (int)format->nb_chapters; }
void km_probe_chapter(AVFormatContext *format,int i,int64_t *id,int64_t *start,int64_t *end,int *num,int *den) {
    AVChapter *c=format->chapters[i];
    *id=c->id;*start=c->start;*end=c->end;*num=c->time_base.num;*den=c->time_base.den;
}
const char *km_probe_chapter_title(Km *k,AVFormatContext *format,int i) {
    const AVDictionaryEntry *entry=k->av_dict_get(format->chapters[i]->metadata,"title",NULL,0);
    return entry?entry->value:"";
}
static int read_mux_packet(Km *k,AVFormatContext *f,AVPacket *p,int stream) {
    for(;;){
        int ret=k->av_read_frame(f,p);
        if(ret==AVERROR_EOF)return 0;
        if(ret<0)return fail(k,ret,"read mux input");
        if(p->stream_index==stream) {
            if(p->pts==AV_NOPTS_VALUE || p->dts==AV_NOPTS_VALUE || p->duration<=0)
                return fail(k,AVERROR_INVALIDDATA,"mux input has missing timing");
            return 1;
        }
        k->av_packet_unref(p);
    }
}
/* HEVC delivery stores parameter sets in hvcC, with a fixed hvc1 sample entry. */
uint32_t km_mux_video_tag(int profile) { return (profile==3 || profile==5)?MKTAG('h','v','c','1'):0; }
/* Chapter markers cross the boundary as ticks in the fixed 1/48000 master
 * clock with a NUL-terminated title; the muxer rescales to each container's
 * own chapter timebase. */
typedef struct { int64_t start; int64_t end; const char *title; } KmChapter;
int km_mux_av(Km *k,const char *video,const char *audio,const char *path,
    const char *render_hash,const char *export_hash,int profile,int audio_channels,
    const KmChapter *chapters,int chapter_count) {
    AVFormatContext *input[2]={NULL,NULL},*out=NULL;AVPacket *packets[2]={NULL,NULL};
    AVStream *streams[2]={NULL,NULL};int index[2]={-1,-1},ready[2]={0,0};
    int ret=0;
    /* Closed profile matrix: 0-7 are the pre-MEDIA-004 movie profiles,
     * 8 = DNxHR/PCM24 MOV and 9 = DNxHR/PCM24 MXF delivery remuxes. */
    const enum AVCodecID videos[10]={AV_CODEC_ID_PRORES,AV_CODEC_ID_AV1,AV_CODEC_ID_H264,AV_CODEC_ID_HEVC,
        AV_CODEC_ID_H264,AV_CODEC_ID_HEVC,AV_CODEC_ID_AV1,AV_CODEC_ID_AV1,AV_CODEC_ID_DNXHD,AV_CODEC_ID_DNXHD};
    const enum AVCodecID audios[10]={AV_CODEC_ID_PCM_S24LE,AV_CODEC_ID_ALAC,AV_CODEC_ID_ALAC,
        AV_CODEC_ID_ALAC,AV_CODEC_ID_AAC,AV_CODEC_ID_AAC,AV_CODEC_ID_AAC,AV_CODEC_ID_OPUS,
        AV_CODEC_ID_PCM_S24LE,AV_CODEC_ID_PCM_S24LE};
    const char *containers[10]={"mov","mp4","mov","mov","mov","mov","mp4","webm","mov","mxf"};
    /* Only mov/mp4/matroska-family outputs can carry chapters (ADR-0133). */
    const int chapter_ok[10]={1,1,1,1,1,1,1,0,1,0};
    if(profile<0 || profile>9)return fail(k,AVERROR(EINVAL),"unknown movie profile");
    if(chapter_count<0 || (chapter_count>0 && !chapters) || chapter_count>1024)
        return fail(k,AVERROR(EINVAL),"invalid chapter list");
    if(chapter_count>0 && !chapter_ok[profile])
        return fail(k,AVERROR(EINVAL),"container cannot carry chapters");
    if(audio_channels<1 || audio_channels>8)
        return fail(k,AVERROR(EINVAL),"mux audio channel budget");
    const char *paths[2]={video,audio};
    for(int i=0;i<2;++i) {
        input[i]=km_probe_open(k,paths[i]);if(!input[i]){ret=-1;goto done;}
        index[i]=k->av_find_best_stream(input[i],i?AVMEDIA_TYPE_AUDIO:AVMEDIA_TYPE_VIDEO,-1,-1,NULL,0);
        if(index[i]<0){ret=fail(k,index[i],"mux stream missing");goto done;}
        AVStream *s=input[i]->streams[index[i]];
        /* Lossy WebM intermediates may publish no stream duration; packet order is authoritative. */
        if(s->start_time!=0 || s->duration==0 || (s->duration<0 && s->duration!=AV_NOPTS_VALUE) ||
            s->codecpar->codec_id!=(i?audios[profile]:videos[profile])) {
            ret=fail(k,AVERROR_INVALIDDATA,"mux requires zero-origin streams matching the closed movie profile");goto done;
        }
        if(i && (s->codecpar->sample_rate!=48000 ||
            s->codecpar->ch_layout.nb_channels!=audio_channels)) {
            ret=fail(k,AVERROR_INVALIDDATA,"mux requires the declared 48 kHz audio layout");goto done;
        }
        packets[i]=k->av_packet_alloc();if(!packets[i]){ret=fail(k,AVERROR(ENOMEM),"mux packet allocation");goto done;}
    }
    ret=k->avformat_alloc_output_context2(&out,NULL,containers[profile],path);
    if(ret<0 || !out){ret=fail(k,ret<0?ret:AVERROR(ENOMEM),"mux output context");goto done;}
    for(int i=0;i<2;++i) {
        streams[i]=k->avformat_new_stream(out,NULL);if(!streams[i]){ret=fail(k,AVERROR(ENOMEM),"mux stream allocation");goto done;}
        AVStream *s=input[i]->streams[index[i]];
        ret=k->avcodec_parameters_copy(streams[i]->codecpar,s->codecpar);if(ret<0){fail(k,ret,"mux codec parameters");goto done;}
        streams[i]->codecpar->codec_tag=i?0:km_mux_video_tag(profile);streams[i]->time_base=s->time_base;
        /* MXF requires a concrete frame rate; codecpar copies do not carry
         * stream rates, so take the probed average or fall back to the
         * inverse time base. */
        if(!i) {
            streams[0]->avg_frame_rate=s->avg_frame_rate.num>0
                ? s->avg_frame_rate : (AVRational){s->time_base.den,s->time_base.num};
            streams[0]->r_frame_rate=streams[0]->avg_frame_rate;
        }
        if(!i && profile==3 && (!streams[i]->codecpar->extradata || streams[i]->codecpar->extradata_size<=0)) {
            ret=fail(k,AVERROR_INVALIDDATA,"hvc1 requires HEVC global-header parameter sets");goto done;
        }
    }
    k->av_dict_set(&out->metadata,"kronello_render_snapshot_hash",render_hash,0);
    k->av_dict_set(&out->metadata,"kronello_export_snapshot_hash",export_hash,0);
    for(int i=0;i<chapter_count;++i) {
        if(chapters[i].start<0 || chapters[i].end<=chapters[i].start ||
            !chapters[i].title || strlen(chapters[i].title)>1024) {
            ret=fail(k,AVERROR(EINVAL),"invalid chapter entry");goto done;
        }
        /* FFmpeg ≥9: chapters are caller-allocated AVChapter structs; the
         * format context frees the array on avformat_free_context. */
        AVChapter *chapter=k->av_mallocz(sizeof(*chapter));
        if(!chapter){ret=fail(k,AVERROR(ENOMEM),"chapter allocation");goto done;}
        chapter->id=i;chapter->time_base=(AVRational){1,48000};
        chapter->start=chapters[i].start;chapter->end=chapters[i].end;
        if(k->av_dict_set(&chapter->metadata,"title",chapters[i].title,0)<0) {
            k->av_free(chapter);ret=fail(k,AVERROR(ENOMEM),"chapter title");goto done;
        }
        AVChapter **grown=k->av_realloc_array(out->chapters,
            (size_t)out->nb_chapters+1,sizeof(*grown));
        if(!grown){k->av_dict_free(&chapter->metadata);k->av_free(chapter);
            ret=fail(k,AVERROR(ENOMEM),"chapter array");goto done;}
        out->chapters=grown;out->chapters[out->nb_chapters++]=chapter;
    }
    ret=k->avio_open(&out->pb,path,AVIO_FLAG_WRITE);if(ret<0){fail(k,ret,"mux output open");goto done;}
    AVDictionary *options=NULL;char timescale[32];
    if(profile!=7 && profile!=9) {
        snprintf(timescale,sizeof(timescale),"%d",streams[0]->time_base.den);
        k->av_dict_set(&options,"video_track_timescale",timescale,0);
        k->av_dict_set(&options,"movflags","use_metadata_tags",0);
        if(profile)k->av_dict_set(&options,"movie_timescale","48000",0);
    }
    ret=k->avformat_write_header(out,&options);k->av_dict_free(&options);if(ret<0){fail(k,ret,"mux header");goto done;}
    for(int i=0;i<2;++i){ready[i]=read_mux_packet(k,input[i],packets[i],index[i]);if(ready[i]<0){ret=ready[i];goto done;}}
    while(ready[0] || ready[1]) {
        int i=!ready[0]?1:!ready[1]?0:
            (k->av_compare_ts(packets[0]->dts,input[0]->streams[index[0]]->time_base,
                packets[1]->dts,input[1]->streams[index[1]]->time_base)<=0?0:1);
        k->av_packet_rescale_ts(packets[i],input[i]->streams[index[i]]->time_base,streams[i]->time_base);
        packets[i]->stream_index=streams[i]->index;packets[i]->pos=-1;
        ret=k->av_interleaved_write_frame(out,packets[i]);k->av_packet_unref(packets[i]);
        if(ret<0){fail(k,ret,"mux write packet");goto done;}
        ready[i]=read_mux_packet(k,input[i],packets[i],index[i]);if(ready[i]<0){ret=ready[i];goto done;}
    }
    ret=k->av_write_trailer(out);if(ret<0){fail(k,ret,"mux trailer");goto done;}
    ret=k->avio_closep(&out->pb);if(ret<0)fail(k,ret,"mux output close");
done:
    for(int i=0;i<2;++i){k->av_packet_free(&packets[i]);k->avformat_close_input(&input[i]);}
    if(out){if(out->pb)k->avio_closep(&out->pb);k->avformat_free_context(out);}
    return ret;
}

/* MEDIA-004 GIF delivery. Deterministic indexed-color pipeline kept inside
 * the audited shim instead of an avfilter dependency: phase 1 streams RGBA
 * frames through a bounded 5-bit-per-channel histogram (128 KiB fixed);
 * km_gif_palette derives a 256-entry median-cut palette and a nearest-color
 * LUT; phase 2 applies a fixed 8x8 Bayer ordered dither, emits PAL8 frames
 * with a per-frame palette, and encodes with the native gif codec/muxer. */
#define GIF_BINS 32768
#define GIF_COLORS 256
#define GIF_FRAME_BUDGET 36000
typedef struct {
    Km *k; int width,height,fps_num,fps_den; int phase;
    uint32_t hist[GIF_BINS]; uint8_t palette[GIF_COLORS][3]; uint8_t lut[GIF_BINS];
    AVFormatContext *format; AVCodecContext *codec;
    AVPacket *packet; AVStream *stream; AVFrame *frame;
    int64_t frames;
} GifEncoder;
static const uint8_t GIF_BAYER[8][8]={
    {0,32,8,40,2,34,10,42},{48,16,56,24,50,18,58,26},
    {12,44,4,36,14,46,6,38},{60,28,52,20,62,30,54,22},
    {3,35,11,43,1,33,9,41},{51,19,59,27,49,17,57,25},
    {15,47,7,39,13,45,5,37},{63,31,55,23,61,29,53,21}};
GifEncoder *km_gif_open(Km *k,int width,int height,int num,int den) {
    if(width<=0 || width>8192 || height<=0 || height>8192 ||
        num<=0 || den<=0 || num>48000 || den>48000) {
        fail(k,AVERROR(EINVAL),"invalid GIF geometry");return NULL;
    }
    GifEncoder *e=calloc(1,sizeof(*e));
    if(!e){fail(k,AVERROR(ENOMEM),"GIF allocation");return NULL;}
    e->k=k;e->width=width;e->height=height;e->fps_num=num;e->fps_den=den;
    return e;
}
int km_gif_frame(GifEncoder *e,const uint8_t *rgba) {
    Km *k=e->k; if(e->phase!=0)return fail(k,AVERROR(EINVAL),"GIF histogram closed");
    if(e->frames>=GIF_FRAME_BUDGET)return fail(k,AVERROR(ENOSPC),"GIF frame budget");
    size_t n=(size_t)e->width*e->height;
    for(size_t i=0;i<n;++i) {
        const uint8_t *p=rgba+i*4;
        e->hist[((p[0]>>3)<<10)|((p[1]>>3)<<5)|(p[2]>>3)]++;
    }
    e->frames++; return 0;
}
/* Median-cut over the histogram: boxes split at population medians along the
 * widest channel; ties resolve to the lowest index so the palette is a pure
 * function of the histogram. Sort keys pack the split channel into the high
 * bits so no comparator state crosses threads. */
static int gif_key_cmp(const void *a,const void *b) {
    uint32_t x=*(const uint32_t*)a,y=*(const uint32_t*)b;
    return x<y?-1:x>y;
}
/* Sort order[start..end) by the histogram channel at bit shift `shift`.
 * `keyed` is caller scratch holding at least GIF_BINS entries. */
static void gif_sort(uint16_t *order,int start,int end,uint32_t *keyed,int shift) {
    int n=end-start;
    for(int i=0;i<n;++i) {
        uint32_t bin=order[start+i];
        keyed[i]=(bin>>shift&31)<<15|bin;
    }
    qsort(keyed,n,sizeof(*keyed),gif_key_cmp);
    for(int i=0;i<n;++i)order[start+i]=(uint16_t)(keyed[i]&32767);
}
int km_gif_palette(GifEncoder *e) {
    Km *k=e->k; if(e->phase!=0)return fail(k,AVERROR(EINVAL),"GIF palette already built");
    if(e->frames==0)return fail(k,AVERROR_INVALIDDATA,"GIF requires at least one frame");
    uint32_t *keyed=k->av_mallocz(sizeof(*keyed)*GIF_BINS);
    if(!keyed)return fail(k,AVERROR(ENOMEM),"GIF sort scratch");
    uint16_t order[GIF_BINS];int unique=0;
    for(int i=0;i<GIF_BINS;++i)if(e->hist[i])order[unique++]=i;
    struct {int start,end;} boxes[GIF_COLORS];int nboxes=0;
    boxes[nboxes++]=(typeof(boxes[0])){0,unique};
    while(nboxes<GIF_COLORS) {
        int best=-1;uint32_t bestpop=0;
        for(int b=0;b<nboxes;++b) {
            if(boxes[b].end-boxes[b].start<2)continue;
            uint32_t pop=0;for(int i=boxes[b].start;i<boxes[b].end;++i)pop+=e->hist[order[i]];
            if(pop>bestpop){bestpop=pop;best=b;}
        }
        if(best<0)break;
        int lo[3]={31,31,31},hi[3]={0,0,0};
        for(int i=boxes[best].start;i<boxes[best].end;++i) {
            int bin=order[i];
            for(int c=0;c<3;++c){int v=(bin>>((2-c)*5))&31;
                if(v<lo[c])lo[c]=v; if(v>hi[c])hi[c]=v;}
        }
        int ch=0,range=-1;
        for(int c=0;c<3;++c)if(hi[c]-lo[c]>range){range=hi[c]-lo[c];ch=c;}
        gif_sort(order,boxes[best].start,boxes[best].end,keyed,(2-ch)*5);
        uint32_t half=bestpop/2,acc=0;int mid=boxes[best].start;
        while(mid<boxes[best].end-1 && acc+ e->hist[order[mid]]<half)acc+=e->hist[order[mid++]];
        if(mid<=boxes[best].start)mid=boxes[best].start+1;
        boxes[nboxes]=(typeof(boxes[0])){mid,boxes[best].end};boxes[best].end=mid;nboxes++;
    }
    for(int b=0;b<nboxes;++b) {
        uint64_t sr=0,sg=0,sb=0,tot=0;
        for(int i=boxes[b].start;i<boxes[b].end;++i) {
            int bin=order[i];uint32_t n=e->hist[bin];
            sr+=(uint64_t)n*(((bin>>10)&31)*8+4);sg+=(uint64_t)n*(((bin>>5)&31)*8+4);
            sb+=(uint64_t)n*((bin&31)*8+4);tot+=n;
        }
        e->palette[b][0]=(uint8_t)((sr+tot/2)/tot);
        e->palette[b][1]=(uint8_t)((sg+tot/2)/tot);
        e->palette[b][2]=(uint8_t)((sb+tot/2)/tot);
    }
    for(int b=nboxes;b<GIF_COLORS;++b){e->palette[b][0]=e->palette[b][1]=e->palette[b][2]=0;}
    for(int i=0;i<GIF_BINS;++i) {
        int r=((i>>10)&31)*8+4,g=((i>>5)&31)*8+4,bl=(i&31)*8+4;
        int best=0;int64_t bestd=INT64_MAX;
        for(int c=0;c<GIF_COLORS;++c) {
            int64_t dr=r-e->palette[c][0],dg=g-e->palette[c][1],db=bl-e->palette[c][2];
            int64_t d=dr*dr+dg*dg+db*db;if(d<bestd){bestd=d;best=c;}
        }
        e->lut[i]=(uint8_t)best;
    }
    k->av_free(keyed);
    e->phase=1; return 0;
}
int km_gif_encode_start(GifEncoder *e,const char *path) {
    Km *k=e->k;
    if(e->phase!=1)return fail(k,AVERROR(EINVAL),"GIF palette required");
    const AVCodec *codec=k->avcodec_find_encoder_by_name("gif");
    if(!codec)return fail(k,AVERROR_ENCODER_NOT_FOUND,"encoder unavailable");
    int ret=k->avformat_alloc_output_context2(&e->format,NULL,"gif",path);
    if(ret<0 || !e->format)return fail(k,ret<0?ret:AVERROR(ENOMEM),"output context");
    e->codec=k->avcodec_alloc_context3(codec);e->packet=k->av_packet_alloc();
    e->frame=k->av_frame_alloc();e->stream=k->avformat_new_stream(e->format,NULL);
    if(!e->codec || !e->packet || !e->frame || !e->stream)
        {fail(k,AVERROR(ENOMEM),"encoder allocation");return -1;}
    e->codec->width=e->width;e->codec->height=e->height;
    e->codec->time_base=(AVRational){e->fps_num,e->fps_den};
    e->codec->framerate=(AVRational){e->fps_den,e->fps_num};
    e->codec->pix_fmt=AV_PIX_FMT_PAL8;e->codec->thread_count=1;
    ret=k->avcodec_open2(e->codec,codec,NULL);
    if(ret<0){fail(k,ret,"open GIF encoder");return -1;}
    e->stream->time_base=e->codec->time_base;
    ret=k->avcodec_parameters_from_context(e->stream->codecpar,e->codec);
    if(ret<0){fail(k,ret,"GIF parameters");return -1;}
    e->frame->format=AV_PIX_FMT_PAL8;e->frame->width=e->width;e->frame->height=e->height;
    ret=k->av_frame_get_buffer(e->frame,1);
    if(ret<0){fail(k,ret,"GIF frame allocation");return -1;}
    ret=k->avio_open(&e->format->pb,path,AVIO_FLAG_WRITE);
    if(ret<0){fail(k,ret,"open GIF output");return -1;}
    AVDictionary *options=NULL;k->av_dict_set(&options,"loop","0",0);
    ret=k->avformat_write_header(e->format,&options);k->av_dict_free(&options);
    if(ret<0){fail(k,ret,"GIF header");return -1;}
    e->frames=0;e->phase=2;return 0;
}
static int gif_drain(GifEncoder *e) {
    Km *k=e->k;int ret;
    while((ret=k->avcodec_receive_packet(e->codec,e->packet))>=0) {
        /* The GIF muxer pins the stream time base to 1/100 (centisecond
         * frame delays); packet times arrive in the codec time base and
         * must be rescaled or every frame would publish a 1cs delay. */
        k->av_packet_rescale_ts(e->packet,e->codec->time_base,e->stream->time_base);
        e->packet->stream_index=e->stream->index;
        ret=k->av_interleaved_write_frame(e->format,e->packet);
        k->av_packet_unref(e->packet);
        if(ret<0)return fail(k,ret,"GIF packet write");
    }
    return ret==AVERROR(EAGAIN) || ret==AVERROR_EOF ? 0 : ret;
}
int km_gif_encode_frame(GifEncoder *e,const uint8_t *rgba) {
    Km *k=e->k;int ret;
    if(e->phase!=2)return fail(k,AVERROR(EINVAL),"GIF encoding not started");
    if(e->frames>=GIF_FRAME_BUDGET)return fail(k,AVERROR(ENOSPC),"GIF frame budget");
    ret=k->av_frame_make_writable(e->frame);
    if(ret<0){fail(k,ret,"GIF frame writable");return ret;}
    uint8_t *dst=e->frame->data[0];int stride=e->frame->linesize[0];
    for(int y=0;y<e->height;++y) {
        const uint8_t *src=rgba+(size_t)y*e->width*4;uint8_t *row=dst+(size_t)y*stride;
        for(int x=0;x<e->width;++x) {
            int d=(int)GIF_BAYER[y&7][x&7]-31;
            int r=src[x*4]+d,g=src[x*4+1]+d,b=src[x*4+2]+d;
            r=r<0?0:r>255?255:r;g=g<0?0:g>255?255:g;b=b<0?0:b>255?255:b;
            row[x]=e->lut[((r>>3)<<10)|((g>>3)<<5)|(b>>3)];
        }
    }
    uint32_t *pal=(uint32_t *)e->frame->data[1];
    for(int i=0;i<GIF_COLORS;++i)
        pal[i]=0xFF000000u|((uint32_t)e->palette[i][0]<<16)|((uint32_t)e->palette[i][1]<<8)|e->palette[i][2];
    e->frame->pts=e->frames;e->frame->duration=1;
    ret=k->avcodec_send_frame(e->codec,e->frame);
    if(ret<0){fail(k,ret,"GIF frame send");return ret;}
    e->frames++;return gif_drain(e);
}
int km_gif_encode_flush(GifEncoder *e) {
    Km *k=e->k;int ret;
    if(e->phase!=2)return fail(k,AVERROR(EINVAL),"GIF encoding not started");
    ret=k->avcodec_send_frame(e->codec,NULL);
    if(ret<0){fail(k,ret,"GIF flush");return ret;}
    ret=gif_drain(e);if(ret<0)return ret;
    ret=k->av_write_trailer(e->format);if(ret<0){fail(k,ret,"GIF trailer");return ret;}
    ret=k->avio_closep(&e->format->pb);if(ret<0){fail(k,ret,"GIF output close");return ret;}
    e->phase=3;return 0;
}
void km_gif_close(GifEncoder *e) {
    if(!e)return;Km *k=e->k;
    k->av_frame_free(&e->frame);k->av_packet_free(&e->packet);k->avcodec_free_context(&e->codec);
    if(e->format){if(e->format->pb)k->avio_closep(&e->format->pb);k->avformat_free_context(e->format);}
    free(e);
}
