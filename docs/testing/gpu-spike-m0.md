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

| 候補経路 | 実装・検証内容 | 初回 / 再実測、追加スパイク |
|---|---|---|
| CPU → GPU upload | 64×64 RGBA16F、32768 bytes、upload と completion fence、画素照合 | 成功、1.568084 / 1.694250 ms、32768 B / 1 op |
| GPU 内コピー | 同サイズ texture-to-texture、32768 bytes、画素照合 | 成功、623.166 / 1385.916 µs、32768 B / 1 op |
| GPU → CPU readback | 同サイズ、32768 bytes（row padding なし）、map と全画素照合 | 成功、1.054292 / 1.016875 ms、32768 B / 1 op |
| IOSurface → MTLTexture → wgpu | 2×2 BGRA8 単一面を生成、lock 中に既知画素を seed、`Device::as_hal` の同じ MTLDevice で texture 化、`texture_from_raw` / `create_texture_from_hal` で import、shader で RGBA8 に読出し全画素照合 | 成功、125.329375 / 6.688875 ms（shader setup を含む）、確認用 readback 512 B |
| wgpu → IOSurface | imported texture を render target として clear、completion fence の後 lock read で BGRA を照合 | 成功、909.209 / 737.709 µs、staging 転送 0 B |
| CvPixelBufferImport | IOSurface 裏付け BGRA8 CVPixelBuffer → CVMetalTextureCache → HAL → shader、既知画素を完全照合 | M1 / Metal 実測で可、430.951041 ms、readback 512 B / 1 op |
| VideoToolboxDecodeBgra8 | 64×64 H.264 3 frame encode → BGRA8 decode → IOSurface / CV cache / HAL → shader | M1 / Metal 実測で可、106.183208 ms、hardware_decoder=Some(true)、max_error=1、readback 49152 B / 3 op |
| VideoToolboxDecodeNv12Biplanar | 64×64 H.264 3 frame encode → NV12 decode、R8 / RG8 plane import と plane bytes 完全照合 | M1 / Metal 実測で可、242.804792 ms、hardware_decoder=Some(true)、readback 49152 B / 3 op、YCbCr→RGB は未検証 |

IOSurfaceCreate の初回 / 再実測は入力 351.917 / 375.500 µs、出力 100.417 / 76.084 µs。同じ device での MTLTexture + HAL import は入力 93.416 / 87.083 µs、出力 114.125 / 81.000 µs。入力全体時間の初回の大きさを allocation / import の時間だけで説明できない。shader / pipeline 生成や待機を含むが、原因の内訳は未計測。

IOSurface の入力 seed は 16 bytes の CPU 書込みで、外部 producer を模した準備である。zero-copy の対象は既存 IOSurface と wgpu texture の共有であり、素材生成や確認用 readback まで CPU 転送ゼロとする意味ではない。入力確認は GPU→CPU staging readback 512 bytes、出力確認は shared IOSurface の CPU lock read 16 bytes。後者は staging 転送ではなく shared memory の観測で、`TransferStats.gpu_readback_bytes` に混ぜない。

unsafe の前提は、同一 device・固定 2×2 BGRA8 / 1 plane / 1 mip / 1 sample、所有 surface の retain、lock による CPU access、GPU completion 後の CPU read、native resource と descriptor の一致。HAL の drop callback は surface の寿命 token のみを持ち、pixel access を公開しない。別 device・別形式・複数 producer の同期・外部プロセス・YUV / HDR の interop 保証は行っていない。

VideoToolbox は `VTDecompressionSession` の出力 CVPixelBuffer を `CVMetalTextureCacheCreateTextureFromImage` へ接続し、同一 MTLDevice の HAL texture として wgpu に取り込む経路を実装した。追加スパイクで M1 / Metal の BGRA8 と NV12 biplanar の decode / import / readback を確認した。任意の pixel format、色 metadata、外部 producer との同期の保証へ一般化せず、MEDIA-001 / FRAME-001 で拡張する。

## CoreVideo / VideoToolbox 追加スパイク

追加実装は macOS の [`videotoolbox` module](../../crates/kronello-framebridge/src/videotoolbox.rs) に隔離した。`CvPixelBufferImport`、`VideoToolboxDecodeBgra8`、`VideoToolboxDecodeNv12Biplanar` を経路として区別する。形式を指定しない旧 `PathKind::VideoToolbox` と、Linux でのこれらの測定は型付き `UNSUPPORTED_FEATURE` とする。

段階 A は `CVPixelBufferCreate` で IOSurface 裏付け・Metal compatibility=true の 2×2 BGRA8 バッファを作り、CPU lock 中に既知の 4 画素を書き込む。同じ wgpu device の `Device::as_hal` から得た MTLDevice で `CVMetalTextureCache` を作り、CVMetalTexture を HAL import、shader で RGBA8 にサンプリングして全 byte を照合する。呼び出し元の pixel buffer / cache 参照は submission 前に drop し、HAL の drop callback が保持する token に CVPixelBuffer / CVMetalTexture / cache を所有させる。通常の `test_cvpixelbuffer_import_to_gpu` は adapter 不在を成功に変えない。

段階 B は 64×64 の grayscale 4 分割パターン（32 / 96 / 160 / 224）を 3 frame、H.264 にメモリ内 encode し、保持した CMSampleBuffer の format description から decoder を作る。出力に IOSurface / Metal compatibility / BGRA8 を指定し、callback 出力を retain、完了待ち後に IOSurface を検査、段階 A の経路でサンプリングする。BGRA の最大 channel 誤差は 2 byte 以下を要求する。`UsingHardwareAcceleratedVideoDecoder` の boolean と query の OSStatus を記録し、query 失敗を software と推定しない。

BGRA が使えない段階では失敗した `NativeStage` と OSStatus を出力して、NV12 を明示的に試す。R8 / RG8 の各 plane を import し、shader 出力の Y / Cb / Cr bytes を lock read した decoder 出力と完全照合する。YCbCr→RGB は実装しない。BGRA の色比較失敗を NV12 成功で隠さない。NV12 だけを明示実測する ignored test も備える。両 decoder 形式の失敗は段階ごとの `NativeError` を返す。`TransferPath` 互換入口では診断を出した上で `UNSUPPORTED_FEATURE` へ変換する。

session guard は早期失敗でも invalidate を行い、callback context と encode 入力を session teardown まで保持する。出力 CVPixelBuffer と CMSampleBuffer は callback 内で retain し、decode の完了後も texture の HAL token が buffer / CV texture / cache を保持する。unsafe の対象は FFI、retained handle の受け渡し、lock 中の画素アクセス、同一 device の HAL import に限定する。

転送 counters には検証用 staging readback を含める。段階 A は seed の CPU 書込み 16 B を準備として除外し、readback は row padding を含む 512 B / 1 op。段階 B は 3 frame の readback 49152 B / 3 op。CPU seed と codec 内部の形式変換は counters の対象外であり、動画処理全体の CPU 転送ゼロを主張しない。段階 B の elapsed は decode / import / shader / validation を含み、encode は除く。

worker sandbox で macOS build、fmt、workspace clippy、Linux `--all-targets` cross-check、`cargo test -p kronello-framebridge --lib --locked` の診断保持 1 test が成功した。`--lib` は integration の段階 A/B を実行するコマンドではない。2026-10-03 に提供された実測結果は Apple M1 MacBook / macOS 27.0 / Rust 1.95.0 / wgpu 30.0.1 / Metal で取得され、通常の段階 A と明示実行した段階 B の BGRA8 / NV12 のすべてが成功した。両 decode 形式で hardware_decoder=Some(true)、BGRA8 は max_error=1 で許容誤差 2 以下。上の経路表はこの単発実測値を記録している。

```sh
WGPU_BACKEND=metal cargo test -p kronello-framebridge --locked -- --nocapture
WGPU_BACKEND=metal cargo test -p kronello-framebridge --test videotoolbox --locked -- --ignored --nocapture
```

[`tests/videotoolbox.rs`](../../crates/kronello-framebridge/tests/videotoolbox.rs) の段階 B の 2 test は通常実行で ignored。段階 A は通常実行に含まれる。ignored の codec test は実測成功に数えず、必要な実測時に明示実行する。FFmpeg の interop、外部ストリーム、色 metadata からの変換、HDR、producer の並行書込み、性能の定常値はこの追加スパイクの保証外。

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

`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` は worker で成功。sandbox 外の Metal で上記 2 crate の `cargo test` は supervisor が実行し、ログを worker が読んで確認した。追加実装を含む作業ツリーの `cargo test --workspace --locked` 全体の最終統合実行は supervisor が担当する。

今回の文書更新時にも sandbox 内で `cargo test --workspace --locked` を実行した。診断保持と residency 契約は各 1 test 成功したが、`measure_transfer_paths` と `iosurface_import_and_output` は Metal adapter 不在の `ADAPTER_UNAVAILABLE` で失敗し、後続テストは実行されなかった。この結果を workspace 全体の成功とは扱わない。

### Linux / macOS CI

Linux の `cargo check --all-targets` は macOS からの cross-check で成功した。さらに [GitHub Actions run 37072973888](https://github.com/soramikan/kronello/actions/runs/37072973888) は macOS (Apple Silicon) と Linux (Mesa lavapipe) の両ジョブが成功した。対象 revision は `07a78ede6203575085b0a1a4a978a2d98877e8bd`、完了は 2026-10-03（JST）。`gh run view 37072973888 --repo soramikan/kronello --json conclusion,status,headSha,jobs` と `--log` で結果を確認した。

両 OS で `cargo test --workspace --locked` が成功し、FrameBridge の `tests/paths.rs` は各 3 passed / 0 failed / 0 ignored。macOS は IOSurface import / output、Linux は native 経路の型付き非対応を含む。Linux では CPU upload / GPU copy / readback の画素照合を Vulkan / lavapipe 上で実行した。Linux CI は `mesa-vulkan-drivers` / `vulkan-tools`、`WGPU_BACKEND=vulkan`、検出した lavapipe ICD の `VK_DRIVER_FILES`、`XDG_RUNTIME_DIR`、`vulkaninfo` の CPU driver 照合を設定している（[workflow](../../.github/workflows/ci.yml)）。

この run は追加の CoreVideo / VideoToolbox 実装前の revision を検証したもので、今回の作業ツリーの CI 成功や VideoToolbox codec 提供を示すものではない。H.264 decode の成功は上記 M1 実測の結果として扱う。追加実装の codec test は通常実行で ignored であり、CI runner の codec 実測は未確認。固定環境の golden 比較もこの CI 成功には含めない。

## 依存とライセンス

`Cargo.lock` の wgpu 30.0.1 は `MIT OR Apache-2.0`、pollster 1.0.1 は `Apache-2.0/MIT`。`cargo metadata --locked --format-version 1` の 181 packages（全 workspace・全 target を含む）を調べ、ライセンス metadata の欠落・GPL / AGPL 専用依存はなかった。`r-efi` 等の LGPL 選択肢を持つ OR 式は MIT / Apache-2.0 側を選ぶ。Unicode-3.0、ISC、Zlib、BSD、0BSD、Unlicense の表示義務等は release packaging で管理する。この確認は Cargo metadata に基づき、配布バイナリと license notices の監査を代替しない。FFmpeg を追加・リンクしていない。

`Cargo.lock` と `cargo metadata --locked --format-version 1` を再照合した objc2 系の解決版とライセンスは以下のとおり。objc2 suite の本体・framework binding は 0.3.2〜0.6.4（objc2-encode は 4.1.0）で、MIT を選択できる。追加依存は macOS target のみで、Apple の system framework を使う。

| crate | 解決版 | ライセンス | 今回追加 |
|---|---|---|---|
| objc2 | 0.6.4 | MIT | — |
| objc2-encode | 4.1.0 | MIT | — |
| objc2-foundation | 0.3.2 | MIT | — |
| objc2-core-foundation / objc2-core-graphics / objc2-io-surface / objc2-metal / objc2-quartz-core | 各 0.3.2 | Zlib OR Apache-2.0 OR MIT | — |
| objc2-core-video / objc2-core-media / objc2-video-toolbox | 各 0.3.2 | Zlib OR Apache-2.0 OR MIT | 直接依存 |
| objc2-core-audio / objc2-core-audio-types | 各 0.3.2 | Zlib OR Apache-2.0 OR MIT | 推移依存 |

追加した 5 crate のライセンスに GPL / LGPL はない。workspace 全体では既存の `r-efi` 5.3.0 / 6.0.0 が `MIT OR Apache-2.0 OR LGPL-2.1-or-later` を提示するため、「metadata に LGPL が一切ない」とは記録しない。MIT / Apache-2.0 を選択でき、GPL / LGPL 専用依存の追加はない。

## 未確認事項

- VideoToolbox encoder への wgpu 出力投入。wgpu → IOSurface 出力までは確認済みだが、今回の encoder 入力は CPU seed した CVPixelBuffer。
- NV12 の YCbCr→RGB 変換と色精度、色 metadata に基づく変換。
- 10bit / HDR / 4K、外部ストリーム、FFmpeg hwaccel、zero-copy の全形式保証、定常性能。
- 追加の CoreVideo / VideoToolbox 実装を含む revision の CI 実行。run 37072973888 の両 OS 成功は追加実装前の結果。
- CI runner での VideoToolbox codec 提供と実デコード。段階 B は ignored。
- M4 参照機の実測、基準画像採取と閾値校正。M1 候補を基準として登録していない。
