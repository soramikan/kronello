# 12 プラットフォームと依存

## 対象プラットフォーム

[ADR-0015](../adr/0015-macos-first-platform-priority.md) により macOS (Apple Silicon) を先行する。

| プラットフォーム | GPU | デコード / エンコード | GUI | 現在の実装・保証境界 |
|---|---|---|---|---|
| macOS (Apple Silicon) | Metal | FFmpeg / 限定 VideoToolbox resident decode | SwiftUI / AppKit | GPU-003 の SDR BGRA8 / NV12 具体経路を保証。HDR resident は未対応 |
| Windows | D3D12 / Vulkan | FFmpeg software | 未実装（GUI-005、M6） | MEDIA-003 の CLI/MCP と QA-004 の D3D12 software adapter 基準を検証。resident は GPU-004（M6） |
| Linux | Vulkan | FFmpeg software | 未実装（GUI-006、M6） | CLI/MCP と QA-004 の Vulkan software adapter 基準を検証。resident は GPU-005（M6） |

「互換経路」でも結果の意味は同じでなければならない。差が出るのは転送コストと速度であり、`render.explain` で使用経路を報告する。

参照機の候補（性能計測用、[13 品質と性能](13-quality-performance.md)）: M4 Mac mini 32GB / macOS、RTX 4060 Ti 16GB / Windows、Linux / NVIDIA runner。

## 独立 worker の native 境界

JOB-002 の `kronello-platform` は Windows-only native module に `windows-sys 0.61.2`
（MIT OR Apache-2.0）を使い、process 起動・生存確認・no-clobber rename を安全な API で提供する。
workspace lints の mirror は unsafe_code=deny のみ例外で、private native module のみ局所許可する。
jobs / service の unsafe forbid と純粋層の依存境界は維持する
（[ADR-0074](../adr/0074-windows-job-workers-and-process-evidence.md)）。

Windows full CLI/MCP は MEDIA-003 で `LoadLibraryExW` による明示 DLL directory の読み込みと、
MinGW で作った LGPL FFmpeg を MSVC ABI の C shim から利用する経路を実装した。
build-time headers は `KRONELLO_FFMPEG_PREFIX` で固定する。
実装方針は [ADR-0082](../adr/0082-windows-ffmpeg-runtime.md)、実行結果は
[MEDIA-003](../testing/media-003.md) を参照する。Windows 実 CI の受け入れ成功と、その範囲を同記録に固定する。
JOB-002 CI は deterministic test payload で jobs/platform の本番 launch / state / publication を確認し、
Windows の CLI/MCP render を保証した結果として扱わない。
Linux は CLI/MCP 実プロセスを追加実行し、初回CIのJOB-002 evidenceは成功した。
Windows の修正版 JOB evidence も成功済み。breakaway拒否環境では `in_parent_job` として親process終了後は続行できるが、
外側Job Object（CI step / service manager等）の終了で停止する制限を持つ。
未検証経路を保証へ昇格させない。OS 別revision / command / exitの証跡は [JOB-002](../testing/job-002.md) に記録する。

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

H.264 / HEVC のエンコーダーがない環境で要求された場合は `ENCODER_UNAVAILABLE` を返し、代替を案内する。音声付き納品の初期経路は MOV / ProRes + 48 kHz stereo PCM24（FFmpeg native `pcm_s24le`）とする（[ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)）。MEDIA-002 は追加出力に native ALAC を採用した。AAC-LC と Opus は [ADR-0106](../adr/0106-versioned-compressed-delivery-audio.md) で採用し、配布用 LGPL 構成に libopus 1.5.2 を pin した。

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

GPU-001 の `kronello-gpu` は wgpu 30.0.1 / pollster 1.0.1 を使い、矩形・PAM 素材から線形 premultiplied RGBA16F までの最短経路を実装した。CPU upload / GPU 内コピー / GPU→CPU readback を別の `TransferStats` として記録する。`kronello-framebridge` の unsafe native interop は macOS のモジュール内に隔離し、IOSurface の BGRA8 単一面取り込み・出力を検証する。これは M0 時点の範囲であり、現在は RENDER-001 の通常 renderer / Render DAG と GPU-003 の限定 BGRA8 / NV12 resident decode を実装・検証済み。実測結果と制約は [スパイク報告](../testing/gpu-spike-m0.md)、Apple Silicon + Metal 共通基準の golden harness（機種・OS・driver は provenance のみ、[ADR-0047](../adr/0047-apple-silicon-metal-golden.md)）は [比較手順](../testing/golden-comparison.md) を参照。

## VideoToolbox / CoreVideo の M0 実測

macOS target の `kronello-framebridge` に CVPixelBuffer import と H.264 decode のスパイクを実装した。M1 / Metal で、IOSurface 裏付け BGRA8 CVPixelBuffer を同じ MTLDevice の CVMetalTextureCache から wgpu 30.0.1 に取り込み、shader readback を照合した。メモリ内の H.264 3 frame encode / decode は BGRA8 と NV12 biplanar（R8 / RG8）の両経路で成功し、両形式で hardware decoder 使用、BGRA8 は最大 channel 誤差 1 を確認した。CVPixelBuffer / CVMetalTexture / cache は HAL drop token で保持する。当時は NV12 の YCbCr→RGB と wgpu 出力の encoder 投入は未検証だった。前者は M4 GPU-003 で検証済み、後者の GPU 常駐 encoder 投入は現在も保証しない。詳細は [GPU / FrameBridge スパイク](../testing/gpu-spike-m0.md) を参照。

追加の objc2-core-video / objc2-core-media / objc2-video-toolbox と推移依存の objc2-core-audio / objc2-core-audio-types は `Cargo.lock` で各 0.3.2、ライセンスは `Zlib OR Apache-2.0 OR MIT` から MIT を選択できる。wgpu 30.0.1 は `MIT OR Apache-2.0`。Apple の system framework のみを使い、GPL / LGPL 依存と FFmpeg を追加していない。既存 objc2 系を含む解決版・ライセンス一覧は [スパイク報告の依存確認](../testing/gpu-spike-m0.md#依存とライセンス) に記録する。

追加実装前の revision `07a78ede6203575085b0a1a4a978a2d98877e8bd` は [CI run 37072973888](https://github.com/soramikan/kronello/actions/runs/37072973888) で macOS / Linux (Mesa lavapipe) の workspace テストが成功し、FrameBridge の `tests/paths.rs` は両 OS で各 3 passed。Linux の Vulkan 転送経路の実行結果であり、追加の VideoToolbox 実装の CI 検証は含まない。codec test は通常実行で ignored。CI runner の codec 提供は未確認。検証範囲は [スパイク報告](../testing/gpu-spike-m0.md#linux--macos-ci) を参照。

## MEDIA-001 の実装境界

`kronello-media` は render の `VideoDecodeBackend` / `DecodedVideoFrame` 契約を使う backend。純粋層に AVFrame を公開しない。FFmpeg の構造体アクセスを C shim、Rust の unsafe を `ffi.rs` に隔離し、libavutil / libavcodec / libavformat / libswscale / libswresample の共有ライブラリを runtime に動的ロードする。system headers で build した開発用 binary と、配布用 LGPL build の検証を分ける。

[ADR-0048](../adr/0048-media-native-build-and-asset-verification.md) で FFmpeg 9.0.2 / SVT-AV1 4.2.0 / dav1d 1.5.4、configure と source SHA-256、毎回全 hash を確認する素材解決を固定した。`scripts/build_ffmpeg_lgpl.py` と `scripts/native-dependencies.json` が正本。`KRONELLO_FFMPEG_LIB_DIR` は実行時の共有ライブラリ directory、`PKG_CONFIG_PATH` は build 時の headers / ABI の選択。runtime override を指定して失敗した場合に system へ戻らない。

`capabilities.get` は schema_version=1 の media capabilities（FFmpeg version、canonical library directory、substituted、各 library の version / license / configuration、distribution_eligible / development_only、検出 codec と compiled hwaccel type）を返す。compiled hardware type の存在は physical device の利用成功を意味しない。GPL / nonfree は development_only とし、`verify_distribution` は LGPL と FFmpeg 9 と必須 AV1 / ProRes を要求する。Ubuntu の distribution FFmpeg / libav*-dev は開発・CI 専用として扱う。

CFR / VFR / B-frame は次 PTS を presentation interval の上端とし、平均 fps や decode 順の DTS で frame を選ばない。[ADR-0091](../adr/0091-exact-forward-decoder-and-bounded-render-scope.md) は現在区間と次 frame を保持し、順方向要求を継続 decode、逆方向要求を exact origin restart で扱う。負の origin は同じ file / stream の再 open で保存し、0 に丸めない。service は sequence / movie / worker の処理範囲で最大 2 decoder・128 MiB の presentation planes を保持し、区間 hit でも素材 hash と lock を検証する。snapshot / 純粋モデルには native handle を入れない。

NLE-002 の SDR RGBA8 変換と明示 GPU upload は [ADR-0062](../adr/0062-video-generator-and-timeline-edits.md)、hardware decode / GPU resident media は [ADR-0081](../adr/0081-guaranteed-metal-hardware-video-decode.md) に従う。HDR は [ADR-0086](../adr/0086-rec2100-native-precision-and-fixed-hdr-output.md) により native 10-bit PQ/HLG の tags と精度を保持して RGBA64 から linear Rec.2020 へ変換する。strict resident HDR は型付き未対応のまま。

AV1 / ProRes の software encode と VideoToolbox H.264 / HEVC encode は公開 enum から選ぶ。SDR 入力は opaque BT.709 RGBA8。M4 COLOR-001 の明示 HDR ProRes profile は高精度 RGBA64 と PQ / HLG の契約を別途持つ。SDRではBT.709 matrix を明示して native YUV に変換し、MOV / MP4 の track timescale によって rational PTS を保持する。path report の CPU copy / conversion / upload counters は logical payload bytes であり、driver の内部転送・待機の実測と区別する。

VideoToolbox の codec 登録は `AV_CODEC_CAP_HYBRID` を含めて検出し、open 時に `allow_sw=0` を指定する。`ENCODER_UNAVAILABLE` は encoder 名と理由、native 初期化失敗時には `FfmpegErrorDetail`（元の戻り値、処理名、`av_strerror` の説明）を保持する。`avcodec_open2` 失敗では pixel format・寸法・time_base も処理名に記録する。

Project の `assets` は stable AssetId、SHA-256 content_hash、kind、rational stream metadata、relative / absolute locator を持つ。未知 Asset は opaque のまま保持する。共有 service の `asset.relink` は base_revision を照合して hash 一致だけを更新し、`project.collect` は store で保存したプロジェクトコピーと素材の相対パス directory を生成する。

再現手順と検証の実施範囲は [MEDIA-001](../testing/media-001.md) に記録する。

## AUDIO-000 の音声境界

`kronello-audio` の純粋な 48 kHz stereo f32 Bus と、media の native decode / libswresample / PCM24 encode / MOV mux を実装する。libswresample は他の 4 library と同じ directory / ABI policy で動的ロードし、capabilities は全 5 library の license / configuration と PCM24 codec を検証する。mono は等倍複製、stereo は保持、多チャンネルの暗黙 downmix は拒否する。AUDIO-010 以降、チャンネルレイアウトは `channel_mask`（mono / stereo / 5.1 / 7.1 の closed set）として decode → mix → encode の全段で保持し、mono はそのまま mono で、>2ch で mask 未指定の素材は `UNSUPPORTED_CHANNEL_LAYOUT` を返す（明示 downmix のみ。[ADR-0124](../adr/0124-pitch-preserving-retime-and-multichannel.md)）。同梱 build は ADR-0106 の libopus も配布 manifest に含め、FFmpeg を `--enable-libopus` で構成する。ビルドスクリプトの verify-only は全 5 library と PCM24 に加え、libopus の同梱と encoder 登録を調べる。配布物の受け入れでは、再配置後の media capabilities の verify_distribution と codec roundtrip も実行する。

音量・量子化・snapshot / 時間の詳細は [基本音声](audio-000.md)、crate 単位の検証と host の残り範囲は [AUDIO-000 の検証](../testing/audio-000.md) を参照。

## MEDIA-002 の追加出力

共有同期 export / job に `av1_mp4` / `h264_mov` / `hevc_mov` の明示 version 1 を追加した。
AV1 は SVT-AV1 software 固定、MP4 + ALAC。FFmpeg 9.0.2 は AV1 の MOV mux を拒否する。
H.264 / HEVC は VideoToolbox `allow_sw=0`、MOV + ALAC、device 不在 / open 失敗は
ENCODER_UNAVAILABLE。HEVC version 1 は `hvc1` sample entry と global-header parameter sets の
`hvcC` を使う（未リリース profile の契約確定、版は維持）。ALAC は既存 native LGPL encoder、48 kHz stereo、PCM24 量子化後の
lossless / zero priming / exact final samples。build flags / native manifest は変更しない。
全5 library の LGPL構成を示す既存 runtime と system development FFmpeg を分けて検証する。
AAC の distribution / patent review・品質評価と AV1 の Opus Web 配信は [ADR-0106](../adr/0106-versioned-compressed-delivery-audio.md) で採用・検証済み（[AUDIO-005](../testing/audio-005.md)）。player compatibility、
この MEDIA-002 version 1 配信用 profile の HDR は未検証・未採用。COLOR-001 の HDR ProRes profile とは区別する。契約は [ADR-0068](../adr/0068-versioned-delivery-movie-profiles.md)、
実行証拠と順序付き host 残件は [MEDIA-002](../testing/media-002.md)。

## 現在の配布・native route の残件

RELEASE-001 は macOS CLI/MCP directory の ad-hoc 署名、再配置、実起動、LGPL runtime 差し替えと codec roundtrip を受け入れた。Developer ID / notarization / Gatekeeper と GUI を含む公開製品配布は未検証（RELEASE-002、M6）。Windows の開発・CI runtime 成功は製品 installer の検証ではなく RELEASE-003（M6）、Linux package は RELEASE-004（M6）で追跡する。詳細は [RELEASE-001](../testing/release-001.md)。

`PathKind::VideoToolbox` の generic spike selector は typed unsupported のまま。具体 probe と GPU-003 の `resident::decode_file` を区別する（FRAMEBRIDGE-001、M5）。macOS resident の追加形式 / HDR は GPU-006（M6）で追跡し、現在の software HDR decode / encode を resident 保証へ読み替えない。
