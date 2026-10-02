# M0 GPU / FrameBridge スパイク（GPU-001）

状態: 実装・検証記録。製品 renderer、HDR 出力、FrameBridge の全形式保証ではない。M4 参照機の基準画像は**未作成・未登録**。

## 実装

- `kronello-gpu`: wgpu 30.0.1 / pollster 1.0.1、CPU 参照の sRGB encode/decode、D65 の線形 Rec.709 ↔ Rec.2020 行列、straight → premultiplied、外部境界の unpremultiply（`alpha_epsilon=2^-16`）、source-over。
- GPU 経路: adapter/device → straight RGBA32F の素材 upload → WGSL で sRGB 復号・原色変換・premultiply → RGBA16F ping-pong 合成 → GPU 内 texture copy → staging buffer readback。矩形、8bit RGB_ALPHA PAM、原点回りの degree 回転と平行移動を扱う。左上原点、画素中心 `(x+0.5,y+0.5)`、nearest sampling、半開矩形。`RenderSize` は `design_extent` と `output_resolution` を分離し、出力画素中心を設計座標へ写してから逆変換する。設計寸法を保った 1x / 2x 出力を比較する。
- CPU / GPU は独立した演算実装。同一 metadata と全画素を `kronello-testkit::compare_pixels` の既定値（`2^-10`）で比較する。範囲外の線形 RGB を clamp せず、非有限出力はエラー。非対応 PAM、backend、device limits 超過等は型付き `UNSUPPORTED_FEATURE` / `INVALID_INPUT`。adapter 不在は `ADAPTER_UNAVAILABLE`、CPU fallback や skip はない。
- `TransferStats` は CPU upload / GPU 内コピー / GPU→CPU readback の bytes と操作数を区別する。`cpu_upload_pixel_bytes` は素材、`cpu_upload_control_bytes` は uniform を数え、GPU 初期ゼロ化や compute 書込みは転送に数えない。readback bytes は row padding を含み、返す RGBA16F は padding を除く。
- `kronello-framebridge`: `PathKind` と `TransferPath`、CPU 往復を `require_gpu_resident` で拒否する契約。ここでの residency 判定は経路の許否であり、adapter の実行能力を宣言するものではない。native handles と unsafe は macOS module に隔離し、純粋モデル層へ依存・GPU 型を追加しない。

## 独立期待値と意味的検証

#F59E0B は `(245,158,11)/255` に sRGB の区分伝達関数を適用し、独立した f64 計算で線形 Rec.709 `(0.9130986517934192, 0.3419144249086609, 0.003346535763899161)` を得る。CPU 参照に定数として掲載した D65 の近似 709→2020 行列で線形 Rec.2020 `(0.6856129627040353, 0.37753480286215557, 0.0480571237962501)` を導出し、テスト内に f32 定数として固定する。oracle から定数を生成しない。両 GPU readback をそれぞれの定数と比較し、2020 readback を 709 に変換した値が 709 readback と同じ色になることを確認する。成分値の差も確認し、タグ付け替えを区別する。

PAM の 4 画素は透明赤、alpha=128/255 の赤、不透明緑、alpha=1/255 の青。sRGB 原色の端点は厳密に 0 / 1 なので、premultiplied 期待値はそれぞれ `(0,0,0,0)`、`(128/255,0,0,128/255)`、`(0,1,0,1)`、`(0,0,1/255,1/255)`。各画素を独立定数と比較し、alpha に gamma を掛けていないことを照合する。

isolated root group は半透明赤と半透明青（各 alpha=0.5）を RGBA16F へ合成し、その結果に group opacity=0.5 を一度掛ける GPU pass を実装した。期待値 `(0.125,0,0.25,0.375)` と、子ごとに opacity を配った誤結果 `(0.1875,0,0.25,0.4375)` を区別する。任意の Scene IR / ネスト group / mask は GPU-002 の実装領域。

+90 degree はローカル +X endpoint を +Y へ写す意味的値と、回転後矩形の画素被覆を独立に照合する。同じ設計座標の矩形を出力解像度 1x / 2x で描画すると幅・高さが 2 倍、面積は 4 倍（6 → 24 画素）、色は同じ。CPU 数学テストは sRGB 区分点の前後、両行列の白色点保存、両方向 round-trip、外部 unpremultiply の閾値を分けて確認する。

## 環境

開発機は MacBookPro17,1 / Apple M1 / 16GB / arm64（supervisor の実測 fingerprint で確認）。worker の `sw_vers` は macOS 27.0（build 26A428）、Rust 1.95.0（59807616e、2026-04-14）、Cargo 1.95.0。Metal driver の個別版は取得できず、OS build と選択 adapter の情報を fingerprint に残す。

worker の sandbox では Metal adapter が列挙されず、GPU テストは `ADAPTER_UNAVAILABLE` で失敗した。これは GPU 成功や転送性能の実測ではない。CPU 参照契約・異常入力・adapter 不在の型付きエラーは 5 test 成功、residency 契約は 1 test 成功。

## 経路と実測

2026-10-03、supervisor が sandbox 外で初回 Metal 実測を行った。adapter は `Apple M1 / IntegratedGpu / Metal / vendor=0 / device=0`、driver / driver_info は空文字。初回 GPU 4 test と FrameBridge 3 test は成功した。追加の単位・group・独立期待値の検証後、再実測で GPU 13 test と FrameBridge 3 test が exit 0 で成功した。通常実行では golden 1 test が意図どおり ignored。以下は 2 回の単発測定で、合否の時間閾値ではない。

各 `SpikePath::measure` は一回の操作を完了 fence まで待って計測する。スパイクの可否を確認するための host elapsed で、GPU timestamp・帯域・定常性能の benchmark ではない。CPU upload / copy の検証用 readback と準備 upload は計測対象・転送 counters から除く。IOSurface は allocation / seed / import / shader setup / 検証を含む全体時間を報告し、allocation と native texture + HAL import の時間も別記する。

| 候補経路 | 実装・検証内容 | 初回 / 再実測 |
|---|---|---|
| CPU → GPU upload | 64×64 RGBA16F、32768 bytes、upload と completion fence、画素照合 | 成功、1.568084 / 1.694250 ms、32768 B / 1 op |
| GPU 内コピー | 同サイズ texture-to-texture、32768 bytes、画素照合 | 成功、623.166 / 1385.916 µs、32768 B / 1 op |
| GPU → CPU readback | 同サイズ、32768 bytes（row padding なし）、map と全画素照合 | 成功、1.054292 / 1.016875 ms、32768 B / 1 op |
| IOSurface → MTLTexture → wgpu | 2×2 BGRA8 単一面を生成、lock 中に既知画素を seed、`Device::as_hal` の同じ MTLDevice で texture 化、`texture_from_raw` / `create_texture_from_hal` で import、shader で RGBA8 に読出し全画素照合 | 成功、125.329375 / 6.688875 ms（shader setup を含む）、確認用 readback 512 B |
| wgpu → IOSurface | imported texture を render target として clear、completion fence の後 lock read で BGRA を照合 | 成功、909.209 / 737.709 µs、staging 転送 0 B |
| VideoToolbox | API 調査のみ。`SpikePath::measure(VideoToolbox)` は `UNSUPPORTED_FEATURE` | decode / encode / CVPixelBuffer / FFmpeg interop は**未実測・未実装** |

IOSurfaceCreate の初回 / 再実測は入力 351.917 / 375.500 µs、出力 100.417 / 76.084 µs。同じ device での MTLTexture + HAL import は入力 93.416 / 87.083 µs、出力 114.125 / 81.000 µs。入力全体時間の初回の大きさを allocation / import の時間だけで説明できない。shader / pipeline 生成や待機を含むが、原因の内訳は未計測。

IOSurface の入力 seed は 16 bytes の CPU 書込みで、外部 producer を模した準備である。zero-copy の対象は既存 IOSurface と wgpu texture の共有であり、素材生成や確認用 readback まで CPU 転送ゼロとする意味ではない。入力確認は GPU→CPU staging readback 512 bytes、出力確認は shared IOSurface の CPU lock read 16 bytes。後者は staging 転送ではなく shared memory の観測で、`TransferStats.gpu_readback_bytes` に混ぜない。

unsafe の前提は、同一 device・固定 2×2 BGRA8 / 1 plane / 1 mip / 1 sample、所有 surface の retain、lock による CPU access、GPU completion 後の CPU read、native resource と descriptor の一致。HAL の drop callback は surface の寿命 token のみを持ち、pixel access を公開しない。別 device・別形式・複数 producer の同期・外部プロセス・YUV / HDR の interop 保証は行っていない。

VideoToolbox は [VTDecompressionSession](https://developer.apple.com/documentation/videotoolbox/vtdecompressionsession-api-collection?language=objc) で session を作り、decoded CVPixelBuffer から [CVMetalTextureCacheCreateTextureFromImage](https://developer.apple.com/documentation/corevideo/cvmetaltexturecachecreatetexturefromimage(_:_:_:_:_:_:_:_:_:)?language=objc) へ接続する API 経路がある。M0 の IOSurface テストからこのデコード経路の性能・形式・寿命を推定しない。MEDIA-001 / FRAME-001 で session、pixel format、plane、色 metadata、解放順、decoder と GPU の同期を検証する。

## golden

[比較手順](golden-comparison.md) の ignored harness を実装した。6 GPU シーン、各 1 frame。UPDATE は CPU 参照画素を検証して `candidate/` を生成し、基準を自動更新しない。M1 候補を M4 基準として採用しない。

| 三段階 | 結果 |
|---|---|
| `KRONELLO_GOLDEN` 未設定 | worker で exit 101、`KRONELLO_GOLDEN=1 required` を確認 |
| UPDATE | 初回・再実測とも exit 0。再実測 `target/golden/update-m1-v3/` に 20 files、candidate は 6 scene / 6 frame。CPU 参照比較成功、`eligible_reference_hardware=false`、`candidate_may_be_adopted=false` |
| 通常比較・基準欠落 | 初回・再実測とも exit 101、`baseline environment.json missing; comparison cannot pass`。最終確認は `target/golden/compare-m1-v3/failure.json` と `report.json` に記録（test=1、scene=6、frame=0） |

## 検証コマンド

worker は sandbox 内の cache 書込み制限のため `CARGO_HOME=/private/tmp/kronello-cargo-home` を指定した。通常環境には不要。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
WGPU_BACKEND=metal cargo test -p kronello-gpu -p kronello-framebridge --locked -- --nocapture
cargo check -p kronello-gpu -p kronello-framebridge --all-targets --target x86_64-unknown-linux-gnu --locked
```

`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` は worker で成功。sandbox 外の Metal で上記 2 crate の `cargo test` は supervisor が実行し、ログを worker が読んで確認した。`cargo test --workspace --locked` 全体の最終統合実行は supervisor が担当する。

Linux の `cargo check --all-targets` は macOS からの cross-check で成功した。Linux 実行・Vulkan/lavapipe の画素比較成功はこの結果から主張しない。Linux CI は `mesa-vulkan-drivers` / `libvulkan1`、`WGPU_BACKEND=vulkan`、lavapipe ICD を必要とする。親が CI に `mesa-vulkan-drivers` / `vulkan-tools`、`WGPU_BACKEND=vulkan`、検出した lavapipe ICD の `VK_DRIVER_FILES` と `XDG_RUNTIME_DIR`、`vulkaninfo` の CPU driver 照合を設定した。CI を worker が編集したものではなく、push 前のため実走は未実施。GPU adapter を確保した上で通常の `cargo test --workspace --locked` に含める。

## 依存とライセンス

crates.io の [wgpu 30.0.1](https://docs.rs/crate/wgpu/30.0.1)、pollster 1.0.1 を確認し lockfile に解決した。`cargo metadata --locked --format-version 1` の 176 packages（全 workspace・全 target を含む）を調べ、ライセンス metadata の欠落・GPL / AGPL 専用依存はなかった。`r-efi` 等の LGPL 選択肢を持つ OR 式は MIT / Apache-2.0 側を選ぶ。Unicode-3.0、ISC、Zlib、BSD、0BSD、Unlicense の表示義務等は release packaging で管理する。この確認は Cargo metadata に基づき、配布バイナリと license notices の監査を代替しない。FFmpeg を追加・リンクしていない。

## 未確認事項

- Linux lavapipe の実行結果、M4 参照機での基準採取と閾値校正は未確認。
- VideoToolbox 実デコード・エンコード、ゼロコピーの全形式保証、定常性能、HDR 出力は後続タスク。
