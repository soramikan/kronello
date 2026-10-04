# ADR-0061: macOS 編集セッションと UI 状態

- 状態: 採用
- 日付: 2026-10-05

## 背景

GUI-001 は ADR-0014 / 0031 / 0056 の native FFI、ADR-0054 / 0055 の画面と部品、
ADR-0026 の selective Undo、ADR-0033 のユーザー状態領域を実アプリへ接続する。
実装とホストの画面・Metal 検証は別の証拠として扱う。

## 決定

- SwiftPM executable `Kronello`、薄い `KronelloAppModel`、既存 `KronelloCore` / `KronelloDesign` を使う。
  View は入力と配置を扱い、`EditorModel` が async Command / Query、通知と UI transaction を扱う。
  MainActor は表示状態を更新し、共有サービス・レンダーは既存 Rust worker、状態ファイル I/O は actor で行う。
  `project.export` / `scene.query` の結果は読取り用の表示 snapshot とし、編集可能な別の作品状態にしない。
  編集は常に共有 strict JSON decoder、`edit.plan` / `edit.apply` / `edit.undo` を通す。
- `SceneNode.name` は optional な表示名、`enabled` は既定 true の作品状態として共有モデルへ追加する。
  `node_rename` / `node_enabled_set` は CLI / MCP / FFI 共通のコマンドで、receipt と Undo を持つ。
  disabled containment subtree は評価・preview・最終レンダーから除く。独立した transform-parent 関係は保つ。
  render snapshot の `SemanticVersions.visibility` は新規 snapshot で 2、旧 absent は 1。
  1 は全ノードが enabled の旧意味に限り実行できる。作品の schema / semantic version 1 と旧保存形式は読める。
- `node_property_insert` は full typed `Property` を追加する共有コマンドとする。
  既存 PropertyId（作品全体）と同じ node の descriptor key の重複を拒否する。
  GUI は未保存の Position / Scale / Rotation の評価既定値を表示し、最初の確定編集だけでこれを使う。
  既存 Property は `property_source_set`、Curve source はその時刻の `keyframe_upsert`、Expression は直接編集不可とする。
- layer lock は作品状態にしない。一人の authoring aid であり、他ユーザーや CLI / MCP の編集権限を変えないためである。
  Layers から locked node を確認できるが、canvas selection / manipulation と Inspector の編集は禁止する。
- `UIStateStore` は `KRONELLO_STATE_ROOT` を優先し、未指定は `~/Library/Application Support/Kronello/`。
  正規化 UUID の `ui-state/<project-id>.json` に page、workspace panel sizes、tool placement、values column、
  pan / zoom、selection、lock、有理数 time を保存する。最近の作品と theme は `preferences.json`。
  この actor は project path / SQLite connection を受け取らず、`.kronello` を書く API を持たない。
  同じディレクトリの一時ファイルと POSIX rename で置換する。time の正本は整数の num / den 文字列。
- drag / NumberField scrub は開始時の revision、node、time を固定する。
  操作中は変換した bounds overlay とローカル field draft を候補として表示し、作品や履歴を変更しない。
  解放時だけ一つの command batch を plan / apply する。候補画素の再レンダーは使わない。
  Scale は authored anchor、Rotation は Option を押した handle drag で扱う。
  overlay は query の AABB を変換した近似で、回転・skew の形状輪郭を再評価した厳密な bounds ではない。
- preview は CAMetalLayer の attach / resize / redraw。画像を JSON / CPU readback へ通さない。
  `ADAPTER_UNAVAILABLE` 等は Viewer を型付きエラーへ置換し、CPU へ自動移行しない。
  FFI に明示 CPU surface preview がないため CPU button は出さない。
  Rectangle / Ellipse / Pen は共有 Shape / Node コマンド。Pen は閉じた自由描画の line segment path。
  Text は既存 document font lock と明示 font input が一致する場合だけ作れる。
  UI 同梱 font の自動登録・作品への暗黙 fallback はしない。multi-span Text 編集は後続範囲。
- `revision_changed` を購読し、project / scene と preview を更新する。異なる revision の query 結果は混ぜず読み直す。
  重複 reload は完了を待つ caller を集約し、古い通知を無視する。成功 apply は再読込完了後に呼出元へ返す。
  selection は stable ID で保持する。外部削除時は選択を解除し、Layers / Dope sheet から消し、Inspector に
  「選択していたレイヤーは削除されました」、操作者 session ID、revision と「履歴で確認…」を出す。
  別の node を自動選択しない。screens/states.md の GUI-001 提案をこの動作で確定する。
  現行 Event は CLI / MCP の表示ラベルを区別しないため、操作者を推測せず session UUID を表示する。
  history のページを追い、削除 Event の session / revision を照合する。該当 Event が保存履歴にない場合は操作者不明の導線と読込 revision を示す。
- session ごとに自分が発行した成功 Event ID だけを新しい順の Undo stack に入れ、永続化・過去履歴からの復元はしない。
  Redo は Undo Event を `edit.undo` する共有意味を使う。失敗時は stack を消費しない。
  `UNDO_CONFLICT` は理由・共有 details / history を sheet、`REVISION_CONFLICT` は候補を保持した Viewer banner。
  再適用はユーザーの明示操作による新しい plan と idempotency key。自動上書き・自動再試行はしない。
- `project.info.open_mode` は実際の store open に由来する `normal` / `safe` を返す。
  **現行 FFI/service は要求ごとに ProjectStore を開閉する。safe mode の排他はその要求中だけであり、
  GUI window 全体の `PROJECT_LOCKED` を保証しない。** band copy にこの制約を明記する。
  session 長の lease / store lifetime は本タスクで変更せず、監督側が後続タスクを管理する。
- Dark を既定とし、Light は明示設定だけで切り替える。全 window root の `krTheme` と `NSApp.appearance` を揃える。
  新規 visual は KronelloDesign と gallery sheet に置く。開発 bundle は executable / FFI / CLI worker / fonts を含め、
  ad-hoc sign する。FFmpeg executable / runtime や GPL binary を組み立てスクリプトで同梱しない。

## 検証と残件

[GUI-001 の検証](../testing/gui-001.md) に criterion ごとの Swift checks、Rust tests、host procedure を記録する。
SwiftPM runner、実 bundle の signing / launch、Metal と両 theme の screenshot review は host 検証が必要。
Curve editor / keyframe 編集（GUI-002）、Sequence 編集（GUI-003）、Template / Export ページ（GUI-004）、
リアルタイム音声（AUDIO-002）、safe-mode session lease はこの実装の完了と区別する。
