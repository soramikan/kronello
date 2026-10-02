# 10 デスクトップ GUI

v0.2 仕様では GUI を Rust crate（`ved-desktop`）としていたが、OS ごとのネイティブフレームワークで実装する方針に改めた（[ADR-0014](../adr/0014-native-gui-in-process-ffi.md)）。GUI の実装は M3 から。

## 構成

```text
apps/macos (Swift: SwiftUI / AppKit)
       |
       |  同一プロセス FFI
       v
cinewright-ffi  --->  cinewright-service (Command / Query API)
                    |
                cinewright-render / cinewright-gpu (wgpu)
                    |
       ネイティブ側が用意した描画面 (CAMetalLayer 等)
```

- ネイティブアプリは Rust コアを同じプロセスにライブラリとして読み込む。
- 編集操作はすべて `cinewright-ffi` 経由で Command / Query API を呼ぶ。GUI 専用の作品状態を作らない。
- プレビューは、ネイティブ側が用意した描画面（macOS では CAMetalLayer）を wgpu の surface として渡して直接描画する。画素を CPU 経由で受け渡さない。

| OS | フレームワーク | 状態 |
|---|---|---|
| macOS | SwiftUI / AppKit | 先行実装（M3） |
| Windows | 候補: WinUI 3 | 未決（[OQ-09](../open-questions.md)） |
| Linux | 候補: GTK4 | 未決（[OQ-09](../open-questions.md)） |

## FFI 境界の規約

- `cinewright-ffi` が公開するのは Command / Query API と、プレビュー面の接続・サイズ変更・再描画要求。
- wgpu、SQLite、Tokio の型を境界に露出しない。
- 重い処理（compile、レンダー、ディスク I/O）は Rust 側の実行系で行い、UI スレッドをブロックしない。結果は通知で返す。
- FFI の生成方式（UniFFI、手書き C ABI など）は未決（[OQ-08](../open-questions.md)）。Windows / Linux からも使える方式を選ぶ。

## UI 状態と作品の分離

選択、pan / zoom、パネル配置、未確定の IME 文字列は UI 状態であり、作品（`.cinewright` の revision）に含めない。

- 未確定の IME 文字列を作品履歴へ大量に commit しない。確定時にコマンドを発行する。
- ドラッグなど連続操作は、操作中はプレビュー用の候補スナップショットで表示し、確定時に一つのコマンドとして発行する。

## Undo

GUI の Undo は、その GUI セッションが発行した操作だけを新しい順に取り消す。エージェントなど別プロセスの変更は取り消さない。対象の Property やオブジェクトが後から変更されている場合は `UNDO_CONFLICT` となり、理由を表示する（[09 保存と同時編集](09-storage-concurrency.md)）。過去のセッションや他の操作者のイベントは、履歴一覧から event ID を指定して取り消す。

## 外部変更への追従

CLI / MCP（エージェント）が同じプロジェクトを編集している場合、GUI は revision の変化を検知して再読込する（[09 保存と同時編集](09-storage-concurrency.md)）。選択中のオブジェクトが外部変更で消えた場合などの UI 上の扱いは GUI-001 で設計する。

## 画面の範囲

| タスク | 内容 |
|---|---|
| FFI-001 | `cinewright-ffi`、Swift からの Command / Query 呼び出し、CAMetalLayer へのプレビュー表示 |
| GUI-001 | Canvas、階層、変換操作、外部変更の検知 |
| GUI-002 | Dope sheet、Curve editor（空間パスと時間イージングを区別して表示） |
| AUDIO-002 | リアルタイム音声再生と A/V 同期 |
| QA-002 | GUI / CLI / MCP の同等性、日本語 IME |
