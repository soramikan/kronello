# kronello-render

Project を固定した RenderSnapshot から Scene IR、Render DAG を構築し、指定有理数時刻・領域の画像と画像連番を生成する。文書は設計座標を保持し、出力解像度は要求だけに指定する。

```rust,ignore
use kronello_render::{
    FrameRequest, OutputRegion, RenderProfile, RenderSnapshot,
    SequenceRequest, render_frame, render_sequence,
};
use kronello_time::{FrameRate, Time, TimeRange};

// The caller resolves project, composition_id, revision, and fonts.
// fonts: &[kronello_text::FontData] supplies bytes verified against locked identities.
let snapshot = RenderSnapshot::new(
    &project, composition_id, revision, RenderProfile::default(),
)?;
let gpu = kronello_gpu::GpuContext::new()?;
let region = OutputRegion {
    origin: [0.0, 0.0],
    extent: [1920.0, 1080.0],
    pixels: [1280, 720],
};
let frame = render_frame(
    &snapshot, fonts, &gpu,
    FrameRequest { time: Time::new(1, 3)?, region },
)?;
let sequence = render_sequence(
    &snapshot, fonts, &gpu,
    SequenceRequest {
        range: TimeRange::new(Time::ZERO, Time::from_integer(1))?,
        frame_rate: FrameRate::new(30000, 1001)?,
        region,
    },
    "fresh-output-directory",
)?;
```

`kronello-render` は GPU / store へ通常依存しない。backend は trait 引数で明示する。CPU 参照は `kronello_gpu::render_adapter::CpuReferenceBackend` を選び、GPU の暗黙 fallback には使わない。

連番は新規 directory に RGBA16F（作業用線形 premultiplied・little endian の数値正本）、16-bit PNG（straight sRGB の閲覧用）、各 frame の JSON と `sequence.json` を書く。既存出力先を上書きしない。異なる実行 backend のビット一致・HDR tone mapping・tiling・GPU texture cache・job resume は保証しない。

型、版、出力形式、制限は [05 レンダラーと GPU](../../docs/architecture/05-render-gpu.md)、受け入れ検証は [RENDER-001](../../docs/testing/render-001.md) を参照。

CACHE-001 は呼出側が所有する `RenderCache` と、scene / DAG / frame / sequence の `*_with_cache` API を追加した。values / layout / geometry / raster は独立した LRU と entry / payload weight の上限を持ち、hit / miss / eviction counter を公開する。layout key には paint・transform を含めず、glyph geometry に layout identity を渡す。CPU 参照 adapter は path の raster 画素を再利用する。GPU raster / texture cache とディスク永続化は未実装。[cache 設計](../../docs/architecture/05-render-gpu.md) と [検証](../../docs/testing/cache-001.md) を参照。
