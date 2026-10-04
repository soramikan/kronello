# 10 デスクトップ GUI

GUI は OS ごとのネイティブフレームワークで実装する（[ADR-0014](../adr/0014-native-gui-in-process-ffi.md)）。macOS の GUI-001 実装を M3 で追加した。画面の受け入れはホストでの検証を要する。

## 構成

```text
apps/macos (Swift: SwiftUI / AppKit)
       |
       |  同一プロセス FFI
       v
kronello-ffi  --->  kronello-service (Command / Query API)
                    |
                kronello-render / kronello-gpu (wgpu)
                    |
       ネイティブ側が用意した描画面 (CAMetalLayer 等)
```

- ネイティブアプリは Rust コアを同じプロセスにライブラリとして読み込む。
- 編集操作はすべて `kronello-ffi` 経由で Command / Query API を呼ぶ。GUI 専用の作品状態を作らない。
- プレビューは、ネイティブ側が用意した描画面（macOS では CAMetalLayer）を wgpu の surface として渡して直接描画する。画素を CPU 経由で受け渡さない。
- GUI ができるまで、Windows / Linux は CLI / MCP を対象とする。Windows / Linux でのプレビュー面の受け渡しは未検証で、各 GUI の着手時にスパイクを行う。

| OS | フレームワーク | 状態 |
|---|---|---|
| macOS | SwiftUI / AppKit | 先行実装（M3） |
| Windows | WinUI 3 | macOS 版の後（[ADR-0032](../adr/0032-windows-winui-linux-gtk.md)） |
| Linux | GTK4 | macOS 版の後（[ADR-0032](../adr/0032-windows-winui-linux-gtk.md)） |

## 見た目

見た目は OS ごとに変えず、[デザインシステム](../design-system/README.md) で全 OS 共通に定める（[ADR-0054](../adr/0054-gui-design-system.md)）。

- 各フレームワークの標準コントロールを、デザインシステムのトークン（色・寸法・書体）でスタイルして使う。IME・アクセシビリティ・キーボード操作は各フレームワークの仕組みを使う。
- メニューバー、ファイルダイアログ、ウインドウの枠など OS が描くものは OS のものを使う。
- テーマは Dark（既定）と Light。書体は Noto Sans JP / Noto Sans Mono を同梱し、アイコンは Lucide を使う。
- 値の正本は [tokens.json](../design-system/tokens.json)。各実装はトークン名を定数名として写し、値を直接書かない。
- メインウインドウは 4 つのページ（編集・モーション・テンプレート・書き出し）で分け、各ページの配置はワークスペースとして保存する（[ADR-0055](../adr/0055-main-window-pages-and-workspaces.md)）。画面ごとの仕様は [画面](../design-system/screens/README.md)。

## FFI 境界の規約

[ADR-0031](../adr/0031-ffi-c-abi-json.md) による。

- `kronello-ffi` は少数の関数からなる C ABI を公開する: プロジェクトを開く・閉じる、Command / Query の呼び出し、通知（revision の変化、ジョブの進捗）の購読、プレビュー面の接続・サイズ変更・再描画要求、メモリの解放。
- Command / Query の要求と応答は、CLI / MCP と同じ JSON で受け渡す。Swift・C#・C の型付きラッパーは公開 JSON Schema から生成する。
- 画素は JSON を通さず、GPU の描画面で直接受け渡す。
- wgpu、SQLite、Tokio の型を境界に露出しない。
- 重い処理（compile、レンダー、ディスク I/O）は Rust 側の実行系で行い、UI スレッドをブロックしない。結果は通知で返す。
- ドラッグ中の連続プレビューなど高頻度の経路での JSON 直列化コストは FFI-001 で計測する。

## FFI-001 の実装範囲

FFI-001 の実装は [ADR-0056](../adr/0056-native-ffi-worker-and-swiftpm.md) に記録した。
`kronello-ffi` は9関数の C ABI、FIFO worker、非 blocking poll、revision / job snapshot 通知、
CAMetalLayer の attach / resize / redraw を持つ。Command / Query は共有 Request decoder / Service に渡し、
作品 path は各要求に明示する。native preview は共有 snapshot / font policy / DAG と GPU lowering を使い、
画素を readback せず SDR surface に描く。SwiftPM の `CKronelloFFI` / `KronelloCore` は実装済みで、
公開 schema 由来の Codable 型と検証専用 `KronelloPreviewHarness` を持つ。
GUI-001 は以下のアプリと UI state 保存、候補 overlay を追加した。
FFI 単体と実アプリの証拠は分け、FFI の確認と境界は [FFI-001 の検証](../testing/ffi-001.md)、
build / ownership は [macOS package README](../../apps/macos/README.md) を参照。

## UI 状態と作品の分離

選択、pan / zoom、パネル配置、未確定の IME 文字列は UI 状態であり、作品（`.kronello` の revision）に含めない。

UI 状態は、ユーザーごとの状態領域にプロジェクト ID で紐付けて保存する（[ADR-0033](../adr/0033-ui-state-in-user-state-area.md)）。`.kronello` には書き込まないため、GUI で開いて眺めただけではプロジェクトファイルは変わらない。別のマシンで開くと表示状態は初期値になる。

- 未確定の IME 文字列を作品履歴へ大量に commit しない。確定時にコマンドを発行する。
- ドラッグなど連続操作は、操作中はプレビュー用の候補スナップショットで表示し、確定時に一つのコマンドとして発行する。

GUI-001 の候補表示は変換した bounds overlay とローカル field draft であり、候補画素は再レンダーしない。
`KronelloAppModel` の `EditorModel` は開始時の revision / time を捕捉し、解放時に一つの plan / apply batch を発行する。
UI state は `KRONELLO_STATE_ROOT` または macOS ユーザー状態領域の `ui-state/<project-id>.json`。
最近の作品・明示 theme は `preferences.json`。layer lock もユーザーごとの UI state とする。
共有作品には optional node name と既定 true の enabled を加え、未保存 Transform の最初の編集には
共有 `node_property_insert` を使う。visibility の snapshot version は 2 とする。
詳細は [ADR-0061](../adr/0061-macos-editor-session-and-ui-state.md)。

## Undo

GUI の Undo は、その GUI セッションが発行した操作だけを新しい順に取り消す。エージェントなど別プロセスの変更は取り消さない。対象の Property やオブジェクトが後から変更されている場合は `UNDO_CONFLICT` となり、理由を表示する（[09 保存と同時編集](09-storage-concurrency.md)）。過去のセッションや他の操作者のイベントは、履歴一覧から event ID を指定して取り消す。

## 外部変更への追従

CLI / MCP（エージェント）が同じプロジェクトを編集している場合、GUI は revision 通知を購読し再読込する（[09 保存と同時編集](09-storage-concurrency.md)）。stable ID が残れば selection を保持する。選択中の node が外部削除された場合は selection を解除し、Inspector に削除通知、session ID / revision と履歴への導線を出す。別の node は自動選択しない。

`project.info.open_mode` で actual store mode を表示する。現行 FFI/service の safe-mode 排他は要求ごとであり、
GUI window の寿命全体を排他にするものではない。32px の band にその制約を表示する。
session 長の `PROJECT_LOCKED` 保証は後続課題とし、本実装で store lifetime を変更しない。

## GUI-001 の実装範囲

`apps/macos` の `Kronello` executable は Welcome、新規作成 / open / recent、4-page toolbar と status、
Motion の Layers / Project、native Viewer、Transform / Text / Layout Inspector、読取り専用 Dope sheet を持つ。
残り3ページは後続タスクを説明する shell。Dark は既定で、OS theme へ自動追従しない。
`scripts/build_macos_app.py` は開発 bundle を組み立て、CLI worker と resource fonts を配置して ad-hoc sign する。
SwiftPM / bundle / Metal と visual fidelity は [GUI-001 の検証](../testing/gui-001.md) の host procedure で確認する。

## 画面の範囲

| タスク | 内容 |
|---|---|
| FFI-001 | `kronello-ffi`、Swift からの Command / Query 呼び出し、CAMetalLayer へのプレビュー表示 |
| GUI-001 | Canvas、階層、変換操作、外部変更の検知 |
| GUI-002 | Dope sheet、Curve editor（空間パスと時間イージングを区別して表示） |
| AUDIO-002 | リアルタイム音声再生と A/V 同期 |
| QA-002 | GUI / CLI / MCP の同等性、日本語 IME |
