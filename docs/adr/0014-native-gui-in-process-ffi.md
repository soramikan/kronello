# ADR-0014: GUI は OS ネイティブフレームワークで実装し、Rust コアを同一プロセス FFI で呼ぶ

- 状態: 採用
- 日付: 2026-10-02

## 背景

v0.2 仕様は GUI を Rust crate（`ved-desktop`）としていたが、フレームワークは未指定だった。日本語 IME、アクセシビリティ、OS 標準の操作感を各 OS で確実に得たい。一方、プレビューの画素をプロセス間や CPU 経由で受け渡すと転送コストが大きい。

## 決定

- GUI は OS ごとのネイティブフレームワークで実装する。macOS は SwiftUI / AppKit を先行する。Windows / Linux は候補（WinUI 3 / GTK4）に留め、後で決める。
- ネイティブアプリは Rust コアを同じプロセスにライブラリとして読み込み、`koma-ffi` 経由で Command / Query API を呼ぶ。
- プレビューは、ネイティブ側が用意した描画面（CAMetalLayer 等）を wgpu の surface として渡して直接描画する。
- `ved-desktop` crate は廃し、`koma-ffi` crate と `apps/<os>/` に置き換える。

## 影響

- OS ごとに GUI を実装する必要があり、GUI の総工数は増える。編集の意味をすべて Rust 側に置くことで重複を UI 層に限定する。
- GUI・CLI・MCP が別プロセスになるため、プロセス間の同時編集の規則が必要になる（ADR-0017）。
- 常駐デーモンがないため、長時間ジョブの実行主体を別途決める必要がある（OQ-03）。
- 検討した代替案: Rust ネイティブ GUI（egui / Iced 等）、Tauri + Web UI、別プロセスのコア + IPC、ハイブリッド。

## 関連

- [10 デスクトップ GUI](../architecture/10-desktop-gui.md)
- [ADR-0017](0017-multi-process-optimistic-concurrency.md)
