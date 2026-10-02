# 12 プラットフォームと依存

## 対象プラットフォーム

[ADR-0015](../adr/0015-macos-first-platform-priority.md) により macOS (Apple Silicon) を先行する。

| プラットフォーム | GPU | デコード / エンコード | 保証水準（M2 時点の目標） |
|---|---|---|---|
| macOS (Apple Silicon) | Metal | VideoToolbox | 第一級。GPU 常駐経路の保証を最初に昇格 |
| Windows | D3D12 / Vulkan | 未定 | 互換経路（CPU 往復を許容）で CI を通す |
| Linux | Vulkan | 未定 | 互換経路（CPU 往復を許容）で CI を通す |

「互換経路」でも結果の意味は同じでなければならない。差が出るのは転送コストと速度であり、`render.explain` で使用経路を報告する。

参照機の候補（性能計測用、[13 品質と性能](13-quality-performance.md)）: M4 Mac mini 32GB / macOS、RTX 4060 Ti 16GB / Windows、Linux / NVIDIA runner。

## FFmpeg

[ADR-0018](../adr/0018-ffmpeg-lgpl-dynamic-linking.md) による。

- libav* を **LGPL 構成** で **動的リンク** する。
- エンコードは OS のハードウェアエンコーダー（macOS では VideoToolbox）を主とする。
- x264 / x265 などの GPL 部品、および nonfree 構成を配布物に含めない。
- 利用者が自分で別構成の FFmpeg に差し替えることは妨げない。実行時に検出した codec / hwaccel を `capabilities.get` で報告し、存在しないものを対応済みと表示しない。
- 通常の API から任意の FFmpeg 引数を渡せるようにしない（[08 API・CLI・MCP](08-api-cli-mcp.md)）。

未決: 同梱かシステムのものを使うか、対応する版の範囲（[OQ-13](../open-questions.md)）。ハードウェアエンコーダーがない環境の既定エンコーダー（[OQ-12](../open-questions.md)）。

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

Cinewright 本体は `MIT OR Apache-2.0`（[ADR-0019](../adr/0019-dual-license-mit-apache.md)）。

- 依存 crate は MIT / Apache-2.0 / BSD 系など、デュアルライセンスと両立するものに限る。GPL / AGPL の依存を追加しない。
- LGPL の FFmpeg は動的リンクとし、利用者が差し替えられる状態を保つ。配布物には FFmpeg のライセンス表示と入手方法を含める。
- H.264 / HEVC などのコーデックには特許ライセンスの論点がある。OS のエンコーダーを使う経路を主とし、それ以外は [OQ-12](../open-questions.md) で扱う。
- フォントとテスト素材の権利確認は [OQ-16](../open-questions.md)。
