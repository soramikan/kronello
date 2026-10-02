# ADR-0032: Windows の GUI は WinUI 3、Linux の GUI は GTK4 とする

- 状態: 採用
- 日付: 2026-10-02

## 背景

ADR-0014 で GUI を OS ネイティブとし、macOS を先行した。Windows / Linux のフレームワークは候補に留めていた。FFI 方式の設計に対象言語を含めるため、方針を確定する。

## 決定

- Windows の GUI は WinUI 3、Linux の GUI は GTK4 で実装する。
- 実装時期は macOS 版（M3）の後とし、それまで Windows / Linux は CLI / MCP を対象とする。

## 影響

- FFI は Swift、C#、C から使えることが要件になる（ADR-0031）。
- プレビュー面の受け渡し（Windows は D3D12 のスワップチェーン、Linux は GTK4 の描画面）の実現性は未検証で、各 GUI の着手時にスパイクが必要。
- バックログには Windows / Linux の GUI タスクをまだ追加していない。

## 関連

- [10 デスクトップ GUI](../architecture/10-desktop-gui.md)
- [ADR-0014](0014-native-gui-in-process-ffi.md)
