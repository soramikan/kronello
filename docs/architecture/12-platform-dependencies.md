# 12 プラットフォームと依存

## 対象プラットフォーム

[ADR-0015](../adr/0015-macos-first-platform-priority.md) により macOS (Apple Silicon) を先行する。

| プラットフォーム | GPU | デコード / エンコード | GUI | 保証水準（M2 時点の目標） |
|---|---|---|---|---|
| macOS (Apple Silicon) | Metal | VideoToolbox | SwiftUI / AppKit（M3） | 第一級。GPU 常駐経路の保証を最初に昇格 |
| Windows | D3D12 / Vulkan | 未定 | WinUI 3（macOS 版の後） | 互換経路（CPU 往復を許容） |
| Linux | Vulkan | 未定 | GTK4（macOS 版の後） | 互換経路（CPU 往復を許容）で CI を通す |

「互換経路」でも結果の意味は同じでなければならない。差が出るのは転送コストと速度であり、`render.explain` で使用経路を報告する。

参照機の候補（性能計測用、[13 品質と性能](13-quality-performance.md)）: M4 Mac mini 32GB / macOS、RTX 4060 Ti 16GB / Windows、Linux / NVIDIA runner。

## FFmpeg

[ADR-0018](../adr/0018-ffmpeg-lgpl-dynamic-linking.md) による。

- libav* を **LGPL 構成** で **動的リンク** する。
- エンコードは OS のハードウェアエンコーダー（macOS では VideoToolbox）を主とする。
- x264 / x265 などの GPL 部品、および nonfree 構成を配布物に含めない。
- 利用者が自分で別構成の FFmpeg に差し替えることは妨げない。実行時に検出した codec / hwaccel を `capabilities.get` で報告し、存在しないものを対応済みと表示しない。
- 通常の API から任意の FFmpeg 引数を渡せるようにしない（[08 API・CLI・MCP](08-api-cli-mcp.md)）。

### 配布

[ADR-0036](../adr/0036-ffmpeg-distribution.md) による。

- リリースの配布物には、版と構成を固定した LGPL ビルドの共有ライブラリを同梱する。
- ビルドスクリプトと native dependencies manifest（版、configure オプション、hash）をリポジトリで管理する。
- 開発時は、pkg-config で見つけたシステムの FFmpeg でもビルドできる。システムの FFmpeg は GPL 構成のことが多いため、リリース前の検証は同梱ビルドで行う。
- 対応する FFmpeg は単一のメジャー版に固定する。
- 利用者は環境変数で別の FFmpeg に差し替えられる。

### macOS 配布パッケージの検証

[ADR-0065](../adr/0065-relocatable-macos-distribution.md) の実装は `scripts/package_macos.py` / `scripts/verify_package.py`。
CLI / MCP と `libavutil.61` / `libavcodec.63` / `libavformat.63` / `libswscale.10` / `libswresample.7`、SVT-AV1 4.2.0 / dav1d 1.5.4 を、固定 native receipt と hash に基づいて同梱する。license / PATENTS は pinned source archive の原文と照合する。FFmpeg executable、headers、static archive、pkg-config や development library は含めない。

package manifest のある CLI / MCP executable は package root の `lib/` を既定ロード先にし、明示 override を優先する。宣言済み package の runtime が壊れている場合は開発 prefix へ戻らない。library は `@loader_path`、executable は `@executable_path/../lib` の依存・rpath とし、全 Mach-O を走査して外部 build prefix / Homebrew 依存を拒否する。

install name の変更後に内側から署名し、元 prefix 外へ copy した package で全署名、CLI / MCP の起動と capabilities、5 本の ABI / LGPL 構成、AV1 と ProRes/PCM24 往復、同一 ABI runtime の差し替えを検証する。ad-hoc の検証と Developer ID / notarization / Gatekeeper の受け入れは区別する。Developer ID 署名では Hardened Runtime と差し替え用 Library Validation exception を executable に付け、notarization は host credential を用いた手動工程とする。実施記録と残件は [RELEASE-001](../testing/release-001.md)。Windows / Linux package は個別の実機検証が完了するまで保証しない。

`bin/` / `lib/` / `tools/` / `licenses/` と manifests の配置を GUI app が後で directory ごと内包できるようにする。GUI app bundle の作成・署名は別タスクであり、本 package の結果を app bundle の保証としない。

### エンコーダー

[ADR-0035](../adr/0035-software-encoders.md) による。

| 用途 | エンコーダー | 提供条件 |
|---|---|---|
| 配信用（H.264 / HEVC） | OS・ハードウェアのエンコーダー（macOS では VideoToolbox） | 利用できる環境のみ |
| 配信用（AV1） | SVT-AV1（ソフトウェア） | 常に。同梱する FFmpeg に含める |
| 中間・納品用 | FFmpeg 内蔵の ProRes | 常に |
| 画像連番 | — | 常に |

H.264 / HEVC のエンコーダーがない環境で要求された場合は `ENCODER_UNAVAILABLE` を返し、代替を案内する。音声付き納品の初期経路は MOV / ProRes + 48 kHz stereo PCM24（FFmpeg native `pcm_s24le`）とする（[ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)）。圧縮音声出力は未実装。

## 主な Rust 依存の候補

採用を確定したものではない。各タスクで要件充足を検証する。

| 領域 | 候補 | 留意点 |
|---|---|---|
| GPU | wgpu | HAL テクスチャ取り込みの安全条件を FrameBridge に隔離 |
| テキスト | Parley / Fontique | 禁則・ルビ・縦書きは個別に検証・補完 |
| ベクター描画 | Vello | `Rgba8Unorm` 前提。HDR 合成へ直結しない |
| 幾何 | kurbo | あらゆる Path 演算を前提にしない |
| テッセレーション | lyon | |
| SVG | usvg | 対応表を持つ。外部参照は自動取得しない |
| 保存 | SQLite | |
| 拡張（M6） | Wasmtime | fuel / epoch、メモリ、host call を別々に制限 |

## ライセンス

Kronello 本体は `MIT OR Apache-2.0`（[ADR-0019](../adr/0019-dual-license-mit-apache.md)）。

- 依存 crate は MIT / Apache-2.0 / BSD 系など、デュアルライセンスと両立するものに限る。GPL / AGPL の依存を追加しない。
- LGPL の FFmpeg は動的リンクとし、利用者が差し替えられる状態を保つ。配布物には FFmpeg のライセンス表示と入手方法を含める。
- H.264 / HEVC などのコーデックには特許ライセンスの論点がある。H.264 / HEVC は OS・ハードウェアのエンコーダーがある場合だけ提供し、ソフトウェアエンコードは AV1 と ProRes とする。
- テスト素材は生成したものと CC0 / 自作に限り、フォントは OFL のものを使う（[ADR-0039](../adr/0039-test-fixtures.md)）。

## M0 GPU スパイクの実装範囲

GPU-001 の `kronello-gpu` は wgpu 30.0.1 / pollster 1.0.1 を使い、矩形・PAM 素材から線形 premultiplied RGBA16F までの最短経路を実装した。CPU upload / GPU 内コピー / GPU→CPU readback を別の `TransferStats` として記録する。`kronello-framebridge` の unsafe native interop は macOS のモジュール内に隔離し、IOSurface の BGRA8 単一面取り込み・出力を検証する。通常 renderer / Render DAG / 他形式の GPU 常駐保証は未実装。実測結果と制約は [スパイク報告](../testing/gpu-spike-m0.md)、Apple Silicon + Metal 共通基準の golden harness（機種・OS・driver は provenance のみ、[ADR-0047](../adr/0047-apple-silicon-metal-golden.md)）は [比較手順](../testing/golden-comparison.md) を参照。

## VideoToolbox / CoreVideo の M0 実測

macOS target の `kronello-framebridge` に CVPixelBuffer import と H.264 decode のスパイクを実装した。M1 / Metal で、IOSurface 裏付け BGRA8 CVPixelBuffer を同じ MTLDevice の CVMetalTextureCache から wgpu 30.0.1 に取り込み、shader readback を照合した。メモリ内の H.264 3 frame encode / decode は BGRA8 と NV12 biplanar（R8 / RG8）の両経路で成功し、両形式で hardware decoder 使用、BGRA8 は最大 channel 誤差 1 を確認した。CVPixelBuffer / CVMetalTexture / cache は HAL drop token で保持する。NV12 の YCbCr→RGB と wgpu 出力の encoder 投入は未検証。詳細は [GPU / FrameBridge スパイク](../testing/gpu-spike-m0.md) を参照。

追加の objc2-core-video / objc2-core-media / objc2-video-toolbox と推移依存の objc2-core-audio / objc2-core-audio-types は `Cargo.lock` で各 0.3.2、ライセンスは `Zlib OR Apache-2.0 OR MIT` から MIT を選択できる。wgpu 30.0.1 は `MIT OR Apache-2.0`。Apple の system framework のみを使い、GPL / LGPL 依存と FFmpeg を追加していない。既存 objc2 系を含む解決版・ライセンス一覧は [スパイク報告の依存確認](../testing/gpu-spike-m0.md#依存とライセンス) に記録する。

追加実装前の revision `07a78ede6203575085b0a1a4a978a2d98877e8bd` は [CI run 37072973888](https://github.com/soramikan/kronello/actions/runs/37072973888) で macOS / Linux (Mesa lavapipe) の workspace テストが成功し、FrameBridge の `tests/paths.rs` は両 OS で各 3 passed。Linux の Vulkan 転送経路の実行結果であり、追加の VideoToolbox 実装の CI 検証は含まない。codec test は通常実行で ignored。CI runner の codec 提供は未確認。検証範囲は [スパイク報告](../testing/gpu-spike-m0.md#linux--macos-ci) を参照。

## MEDIA-001 の実装境界

`kronello-media` は render の `VideoDecodeBackend` / `DecodedVideoFrame` 契約を使う backend。純粋層に AVFrame を公開しない。FFmpeg の構造体アクセスを C shim、Rust の unsafe を `ffi.rs` に隔離し、libavutil / libavcodec / libavformat / libswscale / libswresample の共有ライブラリを runtime に動的ロードする。system headers で build した開発用 binary と、配布用 LGPL build の検証を分ける。

[ADR-0048](../adr/0048-media-native-build-and-asset-verification.md) で FFmpeg 9.0.2 / SVT-AV1 4.2.0 / dav1d 1.5.4、configure と source SHA-256、毎回全 hash を確認する素材解決を固定した。`scripts/build_ffmpeg_lgpl.py` と `scripts/native-dependencies.json` が正本。`KRONELLO_FFMPEG_LIB_DIR` は実行時の共有ライブラリ directory、`PKG_CONFIG_PATH` は build 時の headers / ABI の選択。runtime override を指定して失敗した場合に system へ戻らない。

`capabilities.get` は schema_version=1 の media capabilities（FFmpeg version、canonical library directory、substituted、各 library の version / license / configuration、distribution_eligible / development_only、検出 codec と compiled hwaccel type）を返す。compiled hardware type の存在は physical device の利用成功を意味しない。GPL / nonfree は development_only とし、`verify_distribution` は LGPL と FFmpeg 9 と必須 AV1 / ProRes を要求する。Ubuntu の distribution FFmpeg / libav*-dev は開発・CI 専用として扱う。

CFR / VFR / B-frame は stream start へ seek・flush して前から decode し、次 PTS を presentation interval の上端とする。平均 fps や decode 順の DTS で frame を選ばない。source planes と color tags を返すため PQ / HLG の bit depth は保持する。NLE-002 は明示 stream と SDR RGBA8 の color / linearization / premultiply、CPU sample から選択 GPU への明示 upload を追加した（[ADR-0062](../adr/0062-video-generator-and-timeline-edits.md)）。hardware decode / GPU resident media integration と HDR の working-space 変換は後続の契約。

AV1 / ProRes の software encode と VideoToolbox H.264 / HEVC encode は公開 enum から選ぶ。入力は opaque BT.709 RGBA8。BT.709 matrix を明示して native YUV に変換し、MOV / MP4 の track timescale によって rational PTS を保持する。path report の CPU copy / conversion / upload counters は logical payload bytes であり、driver の内部転送・待機の実測と区別する。

VideoToolbox の codec 登録は `AV_CODEC_CAP_HYBRID` を含めて検出し、open 時に `allow_sw=0` を指定する。`ENCODER_UNAVAILABLE` は encoder 名と理由、native 初期化失敗時には `FfmpegErrorDetail`（元の戻り値、処理名、`av_strerror` の説明）を保持する。`avcodec_open2` 失敗では pixel format・寸法・time_base も処理名に記録する。

Project の `assets` は stable AssetId、SHA-256 content_hash、kind、rational stream metadata、relative / absolute locator を持つ。未知 Asset は opaque のまま保持する。共有 service の `asset.relink` は base_revision を照合して hash 一致だけを更新し、`project.collect` は store で保存したプロジェクトコピーと素材の相対パス directory を生成する。

再現手順と検証の実施範囲は [MEDIA-001](../testing/media-001.md) に記録する。

## AUDIO-000 の音声境界

`kronello-audio` の純粋な 48 kHz stereo f32 Bus と、media の native decode / libswresample / PCM24 encode / MOV mux を実装する。libswresample は他の 4 library と同じ directory / ABI policy で動的ロードし、capabilities は全 5 library の license / configuration と PCM24 codec を検証する。mono は等倍複製、stereo は保持、多チャンネルの暗黙 downmix は拒否する。同梱 build の configure は swresample と native PCM を無効化しておらず、追加の外部 codec 依存はない。ビルドスクリプトの verify-only も全 5 library と PCM24 を調べる。配布物の受け入れでは、再配置後の media capabilities の verify_distribution と codec roundtrip も実行する。

音量・量子化・snapshot / 時間の詳細は [基本音声](audio-000.md)、crate 単位の検証と host の残り範囲は [AUDIO-000 の検証](../testing/audio-000.md) を参照。
