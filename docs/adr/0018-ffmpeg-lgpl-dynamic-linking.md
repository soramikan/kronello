# ADR-0018: FFmpeg は LGPL 構成を動的リンクする

- 状態: 採用
- 日付: 2026-10-02

## 背景

Kronello は MIT OR Apache-2.0 で配布する（ADR-0019）。FFmpeg は構成によって LGPL / GPL / nonfree になり、リンク方法によって配布条件が変わる。外部の ffmpeg 実行ファイルをパイプで使う方式は、フレーム精度の seek や GPU 面の受け渡しが難しい。

## 決定

- libav* を LGPL 構成で動的リンクする。
- エンコードは OS のハードウェアエンコーダー（macOS では VideoToolbox）を主とする。
- x264 / x265 などの GPL 部品と nonfree 構成を配布物に含めない。
- 実行時に検出した codec / hwaccel を `capabilities.get` で報告する。

## 影響

- 利用者が FFmpeg を差し替えられる状態を保つ必要がある。
- ハードウェアエンコーダーがない環境での既定エンコーダーが別途必要（ADR-0035 で決定）。
- 同梱かシステムのものを使うか、対応する版の範囲を決める必要がある（ADR-0036 で決定）。

## 関連

- [12 プラットフォームと依存](../architecture/12-platform-dependencies.md)
