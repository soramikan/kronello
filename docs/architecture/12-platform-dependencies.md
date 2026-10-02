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

### エンコーダー

[ADR-0035](../adr/0035-software-encoders.md) による。

| 用途 | エンコーダー | 提供条件 |
|---|---|---|
| 配信用（H.264 / HEVC） | OS・ハードウェアのエンコーダー（macOS では VideoToolbox） | 利用できる環境のみ |
| 配信用（AV1） | SVT-AV1（ソフトウェア） | 常に。同梱する FFmpeg に含める |
| 中間・納品用 | FFmpeg 内蔵の ProRes | 常に |
| 画像連番 | — | 常に |

H.264 / HEVC のエンコーダーがない環境で要求された場合は `ENCODER_UNAVAILABLE` を返し、代替を案内する。音声コーデックの選定は AUDIO-000 で行う。

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

GPU-001 の `kronello-gpu` は wgpu 30.0.1 / pollster 1.0.1 を使い、矩形・PAM 素材から線形 premultiplied RGBA16F までの最短経路を実装した。CPU upload / GPU 内コピー / GPU→CPU readback を別の `TransferStats` として記録する。`kronello-framebridge` の unsafe native interop は macOS のモジュール内に隔離し、IOSurface の BGRA8 単一面取り込み・出力を検証する。通常 renderer / Render DAG / VideoToolbox デコード・エンコード / 他形式の GPU 常駐保証は未実装。実測結果と制約は [スパイク報告](../testing/gpu-spike-m0.md)、基準未登録の golden harness は [比較手順](../testing/golden-comparison.md) を参照。
