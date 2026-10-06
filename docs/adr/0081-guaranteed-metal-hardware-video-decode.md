# ADR-0081: Metal / VideoToolbox の hardware decode 保証経路

- 状態: 採用
- 日付: 2026-10-06

## 背景

GPU-003 は M0 の import probe や MEDIA-001 の software decode / hardware encode 成功では保証できない。固定 snapshot の映像が hardware decode の出力から同じ GPU device の合成へ到達する経路と、CPU 転送の実測が必要である。

## 決定

- `gpu_resident_bgra8` / `gpu_resident_nv12` は共有 service の明示 backend selection とする。CLI / MCP は `--backend gpu-resident-bgra8` / `--backend gpu-resident-nv12` で同じ選択を使う。既存の `gpu` / `cpu_reference` は明示 software media 経路を維持する。
- `kronello-framebridge` で local regular file を AVFoundation の compressed sample reader へ渡し、`VTDecompressionSession` の `RequireHardwareAcceleratedVideoDecoder=true` を指定する。さらに `UsingHardwareAcceleratedVideoDecoder=true` の実値を確認する。どちらかが保証できなければ typed `UNSUPPORTED_FEATURE`。software へ暗黙 fallback しない。
- 第一保証形式は SDR BT.709 limited-range 8-bit 4:2:0 の H.264 / HEVC (`hvc1` MOV)、出力は IOSurface-backed BGRA8 または NV12 video-range の二 plane とする。HDR、10-bit、full-range、他の matrix、非対応 container / codec は拒否する。実圧縮 format description の色タグと sample entry の `colr` / `avcC` / `hvcC` を確認し、タグ欠落・未指定または bit depth 不明は拒否する。locked metadata の仮定で実素材の HDR / 10-bit を SDR 化しない。
- compressed sample の PTS と root から得た source time を有理数の cross multiplication で比較する。callback 順に依存せず、指定時刻以下の最大 PTS を選ぶ。AVFoundation edit の output PTS は first output PTS を引いて locked stream origin を加える exact rational 演算で補正する。locked stream と native track の `[start,end)` 外は失敗する。callback failure は後続 success で消さない。
- decode callback が pixel buffer を retain し、session wait / invalidate 後も所有する。exact same `MTLDevice` の `CVMetalTextureCache` から import し、HAL drop token が pixel buffer / CVMetalTexture / cache を保持する。各 context には生成時の独立 identity token を置き、別 context の texture を scene input として拒否する。
- BGRA の native chroma conversion と NV12 の nearest chroma / unclipped float を明示的な別経路とし、libswscale の bilinear chroma / RGBA8 clipping と bit-equivalent と呼ばない。GPU shader が BGRA / NV12 の読み出し、BT.709/sRGB inverse transfer、Rec.709→Rec.2020 primaries、affine nearest sampling、premultiplied RGBA16F 化を行う。同じ queue への submit 順で合成との同期を保証する。GPU image の CPU oracle 選択は typed error とし、暗黙 readback しない。
- frame output は要求された最終 linear / display の image と validation status を readback する。resident の保証は decode→color / sample→scene composition の中間経路に適用する。最終出力の readback を zero-copy と呼ばない。GPU shader write は computation、scene の GPU texture copy は copy counter、48-byte sampling uniform は control upload として別に数える。
- `FrameMetadata.transfer_stats` は選択 backend が実行した累積 counter を保存する。`render.explain` の estimate は actual execution と区別し、resident selection の CPU image upload estimate は 0。
- 現在の CPU temporal accumulation と resident 強制は併用を拒否する。対応外は明示 software backend へ選択変更できるが、resident 名義で CPU 往復しない。

## 影響と制限

- model / time / render に native handle を漏らさない。`kronello-render` には backend selection に依存しない transfer counter / resident policy の trait 契約だけを置く。
- asset locator / hash の検証は既存 media resolver を decode 前後に通す。AVFoundation はネットワーク URL や任意 shell の入口にしない。
- compressed demux は最大 262,144 samples / 128 MiB。callback の selected output は一面だけ retain し、全 decoded frame は保持しない。現在は毎要求の compressed clip scan であり、long clip の random access 性能を保証しない。streaming demux / seek は後続最適化で、resource bound の失敗を success に変えない。
- Windows / Linux の resident 保証は未昇格。明示 software decode 経路は維持する。

## 検証

[GPU-003](../testing/gpu-003.md) の native ownership / device / sync と共有 service を通る codec / format 別の比較で確認する。実機未実行の項目は形式保証に数えない。
