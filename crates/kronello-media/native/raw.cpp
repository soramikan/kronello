// Deterministic camera RAW decode over LibRaw (ADR-0136, MEDIA-005).
//
// All LibRaw state stays behind this extern "C" boundary. The Rust side owns
// memory for decoded pixels; the shim only copies into caller buffers.
//
// Decoding contract (pinned here so every platform produces identical output):
//   AHD demosaic (user_qual=3), 16-bit RGB output in sRGB/Rec.709 primaries
//   (output_color=1), linear transfer (gamm=1.0/1.0), camera white balance
//   from AsShotNeutral (use_camera_wb), no content-averaged auto WB, no
//   auto-bright scaling, clipped highlights, file orientation honoured.
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <new>

#include <libraw/libraw.h>
#include <libraw/libraw_internal.h>

struct KrRaw {
    LibRaw raw;
    libraw_processed_image_t *image;
    int last_error;
    int opened;
    int processed;
};

typedef struct KrRawInfo {
    uint32_t raw_width;
    uint32_t raw_height;
    uint32_t width;
    uint32_t height;
    uint32_t iwidth;
    uint32_t iheight;
    uint32_t top_margin;
    uint32_t left_margin;
    int32_t colors;
    uint32_t filters;
    uint32_t dng_version;
    uint32_t raw_count;
    int32_t flip;
    uint32_t tiff_compression;
    uint32_t tiff_bps;
    uint32_t is_foveon;
    float as_shot_neutral[4];
    float cam_mul[4];
    float cam_xyz[4][3];
    float cmatrix[3][4];
    float black;
    float maximum;
    uint32_t dng_color_count;
    char make[64];
    char model[64];
} KrRawInfo;

typedef struct KrRawImage {
    uint32_t width;
    uint32_t height;
    uint32_t colors;
    uint32_t bits;
    uint64_t data_size;
} KrRawImage;

extern "C" {

unsigned kr_raw_capabilities(void) { return LibRaw::capabilities(); }

const char *kr_raw_version(void) { return LibRaw::version(); }

int kr_raw_version_number(void) { return LibRaw::versionNumber(); }

const char *kr_raw_strerror(int code) { return libraw_strerror(code); }

// Returns an opaque handle even when open_file fails so the caller can read
// the typed LibRaw status; check kr_raw_opened before decoding.
void *kr_raw_open(const char *path) {
    KrRaw *k = new (std::nothrow) KrRaw();
    if (!k)
        return nullptr;
    k->image = nullptr;
    k->processed = 0;
    k->opened = 0;
    int ret = k->raw.open_file(path);
    k->last_error = ret;
    if (ret != LIBRAW_SUCCESS)
        return k;
    k->opened = 1;
    return k;
}

int kr_raw_opened(void *ptr) {
    const KrRaw *k = static_cast<const KrRaw *>(ptr);
    return k ? k->opened : 0;
}

int kr_raw_last_error(void *ptr) {
    const KrRaw *k = static_cast<const KrRaw *>(ptr);
    return k ? k->last_error : LIBRAW_UNSPECIFIED_ERROR;
}

int kr_raw_info(void *ptr, KrRawInfo *out) {
    KrRaw *k = static_cast<KrRaw *>(ptr);
    if (!k || !out)
        return LIBRAW_UNSPECIFIED_ERROR;
    if (!k->opened)
        return LIBRAW_OUT_OF_ORDER_CALL;
    const libraw_data_t &d = k->raw.imgdata;
    std::memset(out, 0, sizeof(KrRawInfo));
    out->raw_width = d.sizes.raw_width;
    out->raw_height = d.sizes.raw_height;
    out->width = d.sizes.width;
    out->height = d.sizes.height;
    out->iwidth = d.sizes.iwidth;
    out->iheight = d.sizes.iheight;
    out->top_margin = d.sizes.top_margin;
    out->left_margin = d.sizes.left_margin;
    out->colors = d.idata.colors;
    out->filters = d.idata.filters;
    out->dng_version = d.idata.dng_version;
    out->raw_count = d.idata.raw_count;
    out->flip = d.sizes.flip;
    out->is_foveon = d.idata.is_foveon;
    out->tiff_compression =
        k->raw.get_internal_data_pointer()->unpacker_data.tiff_compress;
    out->tiff_bps = k->raw.get_internal_data_pointer()->unpacker_data.tiff_bps;
    for (int i = 0; i < 4; i++) {
        out->as_shot_neutral[i] = d.color.dng_levels.asshotneutral[i];
        out->cam_mul[i] = d.color.cam_mul[i];
        for (int j = 0; j < 3; j++)
            out->cam_xyz[i][j] = d.color.cam_xyz[i][j];
    }
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 4; j++)
            out->cmatrix[i][j] = d.color.cmatrix[i][j];
    out->black = d.color.black;
    out->maximum = d.color.maximum;
    uint32_t colors = 0;
    for (int i = 0; i < 2; i++) {
        if (d.color.dng_color[i].parsedfields)
            colors++;
    }
    out->dng_color_count = colors;
    std::snprintf(out->make, sizeof(out->make), "%.63s", d.idata.make);
    std::snprintf(out->model, sizeof(out->model), "%.63s", d.idata.model);
    return LIBRAW_SUCCESS;
}

int kr_raw_process(void *ptr, KrRawImage *out) {
    KrRaw *k = static_cast<KrRaw *>(ptr);
    if (!k || !out)
        return LIBRAW_UNSPECIFIED_ERROR;
    if (!k->opened)
        return LIBRAW_OUT_OF_ORDER_CALL;
    if (k->processed)
        return LIBRAW_OUT_OF_ORDER_CALL;
    libraw_output_params_t &p = k->raw.imgdata.params;
    p.user_qual = 3;        // AHD demosaic; LibRaw maps X-Trans to Markesteijn.
    p.output_color = 1;     // sRGB primaries (identical to Rec.709 primaries).
    p.output_bps = 16;      // 16-bit linear output, no 8-bit intermediary.
    p.gamm[0] = 1.0;        // Linear transfer; no display gamma encoding.
    p.gamm[1] = 1.0;
    p.no_auto_bright = 1;   // Never rescale by content statistics.
    p.use_camera_wb = 1;    // White balance from camera AsShotNeutral metadata.
    p.use_auto_wb = 0;      // Never use content-averaged auto white balance.
    p.use_camera_matrix = 1;// Use DNG ColorMatrix/camera profile when present.
    p.highlight = 0;        // Clip highlights; no reconstruction ambiguity.
    p.user_flip = -1;       // Honour the file orientation tag.
    p.no_interpolation = 0;
    p.med_passes = 0;
    p.half_size = 0;
    p.four_color_rgb = 0;
    p.exp_correc = 0;
    p.exp_shift = 0.0f;
    p.user_black = -1;      // Black level from file metadata.
    p.user_sat = -1;        // White level from file metadata.
    int ret = k->raw.unpack();
    if (ret != LIBRAW_SUCCESS) {
        k->last_error = ret;
        return ret;
    }
    ret = k->raw.dcraw_process();
    if (ret != LIBRAW_SUCCESS) {
        k->last_error = ret;
        return ret;
    }
    int err = LIBRAW_SUCCESS;
    k->image = k->raw.dcraw_make_mem_image(&err);
    if (err != LIBRAW_SUCCESS || !k->image) {
        k->last_error = err != LIBRAW_SUCCESS ? err : LIBRAW_UNSPECIFIED_ERROR;
        return k->last_error;
    }
    k->processed = 1;
    out->width = k->image->width;
    out->height = k->image->height;
    out->colors = k->image->colors;
    out->bits = k->image->bits;
    out->data_size = k->image->data_size;
    return LIBRAW_SUCCESS;
}

// Copy the processed image into a caller-owned buffer. `capacity` must be at
// least KrRawImage::data_size. The image is freed after the copy.
int kr_raw_image_copy(void *ptr, uint8_t *dst, uint64_t capacity) {
    KrRaw *k = static_cast<KrRaw *>(ptr);
    if (!k || !dst)
        return LIBRAW_UNSPECIFIED_ERROR;
    if (!k->processed || !k->image)
        return LIBRAW_OUT_OF_ORDER_CALL;
    if (capacity < k->image->data_size)
        return LIBRAW_UNSPECIFIED_ERROR;
    std::memcpy(dst, k->image->data, k->image->data_size);
    return LIBRAW_SUCCESS;
}

void kr_raw_close(void *ptr) {
    KrRaw *k = static_cast<KrRaw *>(ptr);
    if (!k)
        return;
    if (k->image)
        LibRaw::dcraw_clear_mem(k->image);
    delete k;
}

} // extern "C"
