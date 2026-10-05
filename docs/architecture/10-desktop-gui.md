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
画素を readback せず SDR surface に描く。窓が隠れている（`Occluded`）・取得が時間切れの frame は失敗にせず、redraw は `presented:false` と `skipped` を返して描画を省く。表示側は可視になったときに再描画する。`Outdated` は surface を再設定して1回だけ再取得し、それ以外の取得失敗は `SURFACE_ACQUIRE_FAILED`。SwiftPM の `CKronelloFFI` / `KronelloCore` は実装済みで、
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

`project.info.open_mode` で actual store mode を表示する（store を開かない read-only inspection では `read_only_snapshot`）。現行 FFI/service の safe-mode 排他は要求ごとであり、
GUI window の寿命全体を排他にするものではない。32px の band にその制約を表示する。
session 長の `PROJECT_LOCKED` 保証は後続課題とし、本実装で store lifetime を変更しない。

## GUI-001 の実装範囲

`apps/macos` の `Kronello` executable は Welcome、新規作成 / open / recent、4-page toolbar と status、
Motion の Layers / Project、native Viewer、Transform / Text / Layout Inspector を持つ。Dope sheet のキー編集と Curve editor は GUI-002 で追加した。
Edit は GUI-003 で追加した。Template / Export は後続タスクを説明する shell。Dark は既定で、OS theme へ自動追従しない。
`scripts/build_macos_app.py` は開発 bundle を組み立て、CLI worker と resource fonts を配置して ad-hoc sign する。
SwiftPM / bundle / Metal と visual fidelity は [GUI-001 の検証](../testing/gui-001.md) の host procedure で確認する。

## GUI-002 の実装範囲

Dope sheet はクリック / Shift / Command / 矩形選択、フレーム移動と playhead / 他キーへのスナップ、
Navigator diamond の追加 / 削除、Linear / Cubic / Hold の変更を共有 edit batch に渡す。
選択は CurveId + 有理数時刻、ドラッグ開始の revision / keys を捕捉し、候補表示中に作品を書かない。
複数キーの release は一つの plan / apply、一つの Event / Undo。重複時刻は transaction 全体の型付き拒否。
最後のキーの削除は編集した Property だけを共有評価値への Constant Source に変える。
別の Property / Expression CurveSample が参照している場合は Curve とキーを保持し、単独消費者の場合だけキーを remove する。
Source 変更と（単独消費者の場合の）remove は同じ batch / 一回の Undo。

下段の Curve editor は216pxのチャンネル列と値 / 速度グラフ、選択キーと Cubic の接線を持つ。
X / Y は区間の TimeBezier を共有する。揃える / 分けるは導出する UI 補助であり、
揃えるは二つの隣接区間を一つの batch で `keyframe_replace`、分けるは片側だけ。
Expression に編集用の Curve は出さない。速度は表示のみ。Viewer の空間パスは別表示の読取り専用で、
Composition のフレーム（最大600点）と正確なキー時刻を一回の共有 `property.sample` で評価し、
親空間の Position 軌跡を cached presentation として保持する。現在の共有 scene の親 `world_transform`
だけを適用して1px線と5pxキー位置を表示する。祖先が動く場合も playhead の親空間に対する局所軌跡であり、
playhead の移動は親 matrix が変わったときだけ再配置し、再サンプルしない。revision / 選択 / Composition の変更で再評価する。
欠落した scene ノード / 親 matrix / sample は `KRErrorLine` に示す。
Curve editor の速度 readout は Property 単位/秒、表示のみの説明は `ink-muted` の footer に常設する。
readout は軸ラベルの余白を避け、右端で左側へ反転する。focus ring は各 control の所有する `FocusState` を使い、祖先の focus を継承しない。
名前・値表示は `PropertyPresentation`、琥珀 / 青 / 赤の役割と単一 playhead は GUI-001 review を継承する。
詳細は [ADR-0070](../adr/0070-motion-keyframe-authoring.md)、検査とホスト手順は [GUI-002 の検証](../testing/gui-002.md)。

## 画面の範囲

GUI-003 の Edit は `KREditLayout` の Project280px / Viewer / Inspector296px / tracks312px。
共有文書 export と一つの `sequence.query` を revision 照合して採用し、素材ごと・clip ごとの
FFI request は発行しない。`asset_status` は locate/stat だけの存在確認で hash 未検証。
色は種類の icon / 2px 下線、青の選択、amber の現在時刻、欠落の danger 破線・code を使う。
asset の stream を明示して1倍速配置、同一 track 移動・subset trim・共有 ClipSplit を
候補 geometry と release 時一つの Command / Event に接続する。Session Undo / conflicts は GUI-001 と共通。
Composition clip の「モーションで開く」は参照先 Composition への UI navigation だけ。
track mute/visibility は説明付き disabled、lock は UI state。reverse/composite/speed は表示のみと明示する。
Sequence の seek は UI time / preview refresh だけで毎フレームの query を追加しない。
AUDIO-002 が `activatePlayback(for:)` を `configurePlayback(target:.sequence(id),rateNum:,rateDen:)` に接続する。
独自の playback timer は持たない。決定は [ADR-0075](../adr/0075-sequence-edit-page-and-clip-split.md)、
検査・ホスト手順は [GUI-003](../testing/gui-003.md)。

| タスク | 内容 |
|---|---|
| FFI-001 | `kronello-ffi`、Swift からの Command / Query 呼び出し、CAMetalLayer へのプレビュー表示 |
| GUI-001 | Canvas、階層、変換操作、外部変更の検知 |
| GUI-002 | Dope sheet、Curve editor（空間パスと時間イージングを区別して表示） |
| GUI-003 | Sequence tracks / Project / clip Inspector、共有配置・trim・split と Motion navigation |
| AUDIO-002 | リアルタイム音声再生と A/V 同期 |
| QA-002 | GUI / CLI / MCP の同等性、日本語 IME |
