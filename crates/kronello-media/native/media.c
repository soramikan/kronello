/* Audited ABI boundary. FFmpeg resources never leave this file. All libav
 * calls use symbols from explicitly opened shared libraries, never static FFmpeg.
 * Each context owns its libraries and decoder/encoder children. No global state. */
#include <libavformat/avformat.h>
#include <libavcodec/avcodec.h>
#include <libavutil/imgutils.h>
#include <libavutil/pixdesc.h>
#include <libavutil/hwcontext.h>
#include <libswscale/swscale.h>
#include <dlfcn.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>

#define STR_(x) #x
#define STR(x) STR_(x)
typedef struct Km {
    void *libs[4];
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
    __typeof__(&swscale_version) swscale_version;
    __typeof__(&swscale_license) swscale_license;
    __typeof__(&swscale_configuration) swscale_configuration;
    __typeof__(&sws_getContext) sws_getContext;
    __typeof__(&sws_getCoefficients) sws_getCoefficients;
    __typeof__(&sws_setColorspaceDetails) sws_setColorspaceDetails;
    __typeof__(&sws_scale) sws_scale;
    __typeof__(&sws_freeContext) sws_freeContext;
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
    for (int i=3; i>=0; --i) if (k->libs[i]) dlclose(k->libs[i]);
    free(k);
}
const char *km_error(Km *k) { return k->error; }
int km_error_code(Km *k) { return k->error_code; }
const char *km_error_operation(Km *k) { return k->error_operation; }
const char *km_error_detail(Km *k) { return k->error_detail; }
Km *km_open(const char *directory, char *error, size_t capacity) {
    Km *k = calloc(1, sizeof(*k));
    if (!k) { snprintf(error, capacity, "allocation failed"); return NULL; }
    const char *names[4] = {"avutil", "avcodec", "avformat", "swscale"};
    int majors[4] = {LIBAVUTIL_VERSION_MAJOR, LIBAVCODEC_VERSION_MAJOR, LIBAVFORMAT_VERSION_MAJOR, LIBSWSCALE_VERSION_MAJOR};
    for (int i=0; i<4; ++i) {
        char path[4096];
#ifdef __APPLE__
        int n=snprintf(path, sizeof(path), "%s/lib%s.%d.dylib", directory, names[i], majors[i]);
#else
        int n=snprintf(path, sizeof(path), "%s/lib%s.so.%d", directory, names[i], majors[i]);
#endif
        if (n<0 || (size_t)n>=sizeof(path)) { snprintf(error, capacity, "library path too long"); km_close(k); return NULL; }
        k->libs[i]=dlopen(path, RTLD_NOW | RTLD_LOCAL);
        if (!k->libs[i]) { snprintf(error, capacity, "%s: %s", path, dlerror()); km_close(k); return NULL; }
    }
#define LOAD(i, name) do { *(void **)(&k->name) = dlsym(k->libs[i], #name); if (!k->name) { snprintf(error, capacity, "missing symbol: %s", #name); km_close(k); return NULL; } } while(0)
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
    LOAD(3, swscale_version);
    LOAD(3, swscale_license);
    LOAD(3, swscale_configuration);
    LOAD(3, sws_getContext);
    LOAD(3, sws_getCoefficients);
    LOAD(3, sws_setColorspaceDetails);
    LOAD(3, sws_scale);
    LOAD(3, sws_freeContext);
#undef LOAD
    unsigned versions[4] = {k->avutil_version(), k->avcodec_version(), k->avformat_version(), k->swscale_version()};
    for (int i=0;i<4;++i) if ((versions[i]>>16)!=(unsigned)majors[i]) { snprintf(error, capacity, "FFmpeg ABI major mismatch"); km_close(k); return NULL; }
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
        default:return "";
    }
}
unsigned km_version(Km *k, int index) {
    switch(index) {case 0:return k->avutil_version(); case 1:return k->avcodec_version(); case 2:return k->avformat_version(); default:return k->swscale_version();}
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
Decoder *km_decoder_open(Km *k, const char *path) {
    Decoder *d=calloc(1,sizeof(*d));
    if(!d) { fail(k,AVERROR(ENOMEM),"decoder allocation"); return NULL; }
    d->k=k; d->format=k->avformat_alloc_context();
    if(!d->format) goto alloc_failed;
    /* Only local single-file demuxers. Playlists, URL protocols and demuxers
     * that fetch external resources are outside the public media API. */
    AVDictionary *options=NULL;
    k->av_dict_set(&options,"protocol_whitelist","file",0);
    k->av_dict_set(&options,"format_whitelist","nut,matroska,webm,mov,avi,mpegts,mpeg,ogg,wav,png_pipe,jpeg_pipe",0);
    int ret=k->avformat_open_input(&d->format,path,NULL,&options);
    k->av_dict_free(&options);
    if(ret<0) { fail(k,ret,"open input"); goto failed; }
    ret=k->avformat_find_stream_info(d->format,NULL);
    if(ret<0) { fail(k,ret,"stream info"); goto failed; }
    const AVCodec *codec=NULL;
    d->stream=k->av_find_best_stream(d->format,AVMEDIA_TYPE_VIDEO,-1,-1,&codec,0);
    if(d->stream<0) { fail(k,d->stream,"video stream"); goto failed; }
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

typedef struct Encoder {
    Km *k; AVFormatContext *format; AVCodecContext *codec; AVFrame *frame; AVPacket *packet;
    struct SwsContext *sws; AVStream *stream; int header;
} Encoder;
void km_encoder_close(Encoder *e) {
    if(!e)return;
    Km *k=e->k;
    if(e->sws)k->sws_freeContext(e->sws);
    k->av_frame_free(&e->frame); k->av_packet_free(&e->packet); k->avcodec_free_context(&e->codec);
    if(e->format) { if(e->format->pb)k->avio_closep(&e->format->pb); k->avformat_free_context(e->format); }
    free(e);
}
Encoder *km_encoder_open(Km *k, const char *path, const char *name, int width, int height, int num, int den) {
    Encoder *e=calloc(1,sizeof(*e)); if(!e) {fail(k,AVERROR(ENOMEM),"encoder allocation");return NULL;} e->k=k;
    const AVCodec *codec=k->avcodec_find_encoder_by_name(name);
    if(!codec) {fail(k,AVERROR_ENCODER_NOT_FOUND,"encoder unavailable");goto failed;}
    int prores=!strcmp(name,"prores_ks");
    int ret=k->avformat_alloc_output_context2(&e->format,NULL,prores?"mov":"mp4",path);
    if(ret<0 || !e->format) {fail(k,ret<0?ret:AVERROR(ENOMEM),"output context");goto failed;}
    e->codec=k->avcodec_alloc_context3(codec); e->frame=k->av_frame_alloc();e->packet=k->av_packet_alloc();e->stream=k->avformat_new_stream(e->format,NULL);
    if(!e->codec || !e->frame || !e->packet || !e->stream){fail(k,AVERROR(ENOMEM),"encoder allocation");goto failed;}
    e->codec->width=width; e->codec->height=height;e->codec->time_base=(AVRational){num,den};e->codec->framerate=(AVRational){den,num};
    e->codec->pix_fmt=prores?AV_PIX_FMT_YUV422P10LE:AV_PIX_FMT_YUV420P;
    e->codec->color_primaries=AVCOL_PRI_BT709;e->codec->color_trc=AVCOL_TRC_BT709;e->codec->colorspace=AVCOL_SPC_BT709;e->codec->color_range=AVCOL_RANGE_MPEG;
    e->codec->thread_count=1;e->codec->bit_rate=2000000;
    if(e->format->oformat->flags & AVFMT_GLOBALHEADER)e->codec->flags|=AV_CODEC_FLAG_GLOBAL_HEADER;
    AVDictionary *options=NULL;
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
        e->k->av_packet_rescale_ts(e->packet,e->codec->time_base,e->stream->time_base);e->packet->stream_index=e->stream->index;
        ret=e->k->av_interleaved_write_frame(e->format,e->packet);e->k->av_packet_unref(e->packet);
        if(ret<0)return fail(e->k,ret,"write packet");
    }
}
int km_encoder_frame(Encoder *e,const uint8_t *rgba,int64_t pts) {
    int ret=e->k->av_frame_make_writable(e->frame);if(ret<0)return fail(e->k,ret,"writable frame");
    const uint8_t *data[4]={rgba,NULL,NULL,NULL};int stride[4]={e->codec->width*4,0,0,0};
    ret=e->k->sws_scale(e->sws,data,stride,0,e->codec->height,e->frame->data,e->frame->linesize);
    if(ret!=e->codec->height)return fail(e->k,AVERROR_INVALIDDATA,"pixel conversion");
    e->frame->pts=pts;
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
