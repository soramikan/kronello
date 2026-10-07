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
| Windows | WinUI 3 | 未実装。GUI-005（M6）でsurfaceを含め検証（[ADR-0032](../adr/0032-windows-winui-linux-gtk.md)） |
| Linux | GTK4 | 未実装。GUI-006（M6）でsurfaceを含め検証（[ADR-0032](../adr/0032-windows-winui-linux-gtk.md)） |

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

- 作品に結び付く文字列欄は native `NSTextInputClient` の marked text をローカル draft として保持し、変換・候補選択中は plan / apply を発行しない。
  IME の確定（`unmarkText` / marked text を置換する `insertText`）、Return、blur は、marked text がなく値が変わった場合だけ一つの Command を発行する。
  同じ確定に続く Return / blur は重複発行しない。Escape は draft を取り消し、Command を発行しない。
  文字列の変更は UTF-8 bytes で判定し、合成済み文字と結合文字の違いを落とす Unicode 正規化は行わない。
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

`project.info.open_mode` で actual store mode を表示する（store を開かない read-only inspection では `read_only_snapshot`）。
FFI-002 は安全モードの store を native session の生存中保持し、同じ project の共有要求へ貸し出す。
他プロセスの通常 Command / Query は `PROJECT_LOCKED` となる。32px の band に安全モードを表示する。
close 後は受け付け済み処理の終了時に解放し、異常終了では OS が lock を解放する。
通常モードの外部編集と通知は要求ごとの store を維持する。
設計は [ADR-0083](../adr/0083-native-safe-project-session.md)、確認範囲は [FFI-002](../testing/ffi-002.md) を参照する。

## GUI-001 の実装範囲

`apps/macos` の `Kronello` executable は Welcome、新規作成 / open / recent、4-page toolbar と status、
Motion の Layers / Project、native Viewer、Transform / Text / Layout Inspector を持つ。Dope sheet のキー編集と Curve editor は GUI-002 で追加した。
Edit は GUI-003、Template / Export は GUI-004 で追加した。Dark は既定で、OS theme へ自動追従しない。
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

## テンプレート内部の読み取り検査

Motion のテンプレート配置を選ぶと、内部 node を `InstancePath + NodeId` で選び、
評価された Property・text・layout / ink / visual bounds を Inspector に表示する。
内部値は編集 field を持たず、Viewer の青い bounds 枠にも操作 handle を付けない。
Inspector / Viewer は同じ表示 cache を使い、停止時の revision / Composition / 配置 / time
変更を150ms debounceした一回の expanded `scene.query` で更新する。
再生中は最後の値と「再生中は停止時に更新」を出し、inspection request は発行しない。
古い task / revision は破棄し、失敗は `KRErrorLine` を表示する。
[ADR-0077](../adr/0077-template-instance-read-only-inspection.md) と
[INTEGRATION-002 の検証](../testing/integration-002.md) に範囲と証拠を記録する。

## 画面の範囲

AUDIO-002 の playback は [ADR-0076](../adr/0076-buffered-device-clock-playback.md)、
[実デバイスの測定手順](../testing/audio-002.md) に従う。`kronello-service::PreparedAudio` と
producer-only binary C ABI は Metal と同様の preview runtime resource で、process-local handle を
stateless Command / Query registry に追加しない。sample semantics は共有 export evaluator 2。
preparation / producer queues、既存 project / Metal worker、AVAudioSourceNode render thread を分離する。
callback は事前確保した32768-frame lock-free SPSC buffer の copy / silence と timestamp / counters だけ。
MainActor の `EditorModel` が device sample / host timestamp による presentation timer を所有し、
frame ごとの scene / project / history reload を行わない。MetalPreview は最新要求へ集約する。
mute / 音声なし / デバイスなしは host clock と理由を status に示し、underrun と typed error も表面化する。
GUI-003 は typed `configurePlayback(target: .sequence(id), rateNum: ..., rateDen: ...)` を使い、
Motion は nil/default Composition。ページ側の再生 timer は不要。実engine / Metalの後続計測は [AUDIO-002](../testing/audio-002.md)、Dark / Lightの表示・単独focusの実操作は [GUI-002](../testing/gui-002.md) と [M3統合受け入れ](../testing/m3-acceptance.md) を参照する。主観的listening・物理scanout・VoiceOverは受け入れ保証に含めない。

GUI-003 の Edit は `KREditLayout` の Project280px / Viewer / Inspector296px / tracks312px。
共有文書 export と一つの `sequence.query` を revision 照合して採用し、素材ごと・clip ごとの
FFI request は発行しない。`asset_status` は locate/stat だけの存在確認で hash 未検証。
色は種類の icon / 2px 下線、青の選択、amber の現在時刻、欠落の danger 破線・code を使う。
asset の stream を明示して1倍速配置、同一 track 移動・subset trim・共有 ClipSplit を
候補 geometry と release 時一つの Command / Event に接続する。Session Undo / conflicts は GUI-001 と共通。
Composition clip の「モーションで開く」は参照先 Composition への UI navigation だけ。
Viewer error は target / revision / rational time に結ぶ。video の GPU unsupported にだけ
明示「CPU 参照で表示」を提示し、選択した Sequence tab の session 中だけ muted badge を表示する。
CPU は共有 render.frame/backend と media decode を使い、native surface に current-frame pixels を upload する。
一件ずつ最新要求へ集約し、古い completion を破棄する。再生中の CPU 要求は停止し、最後の frame と
stale 注記を表示する。native redraw 自体の中断は未対応。blade の専用 hit area は Button の tap と競合しない。
GUI-003の受け入れ時点ではtrack mute/visibilityは説明付きdisabled、reverse/composite/speedは表示のみだった。M5のGUI-007では共有TrackStateSet / ClipTimeSet / ClipSetEffectsへの接続と逆方向samplingを実装・検証中。lockはUI stateを維持する。実装・個別試験・実GUI受け入れの区別は [GUI-007記録](../testing/gui-007.md)、時間と出力の契約は [ADR-0100](../adr/0100-shared-edit-controls-and-explicit-reverse-sampling.md) を参照する。
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


## GUI-004 の実装範囲

Template は264pxの Templates、中央の variant 比較、320pxの公開入力 / 版、
右列を除く下段196pxの尺ポリシー。`TemplatePageModel` は immutable query の候補・選択・
診断だけを保持し、作品の変更は `EditorModel.apply` の一つの template command を通す。
公開入力は比較用の値と選んだ配置への適用を区別する。layout / ink / visual の切替は
一回取得した `template.preview` の bounds を使う。overflow で nodes が返らない場合は
型付き診断と bounds unavailable を示す。画素や glyph geometry を独自に再評価しない。
保護尺の下書きは新 immutable edition の公開、配置の尺は set_duration、版の移行は
migration_plan の差分確認 → 明示した適用として分け、既存 placement を自動更新しない。

Export は320pxの設定、中央の native Viewer、304pxの確認、設定を除く下段232pxのジョブ。
選択肢は Rust が返す `capabilities.get.export_profiles` の閉じた出力だけを使う。
ハードウェア encoder の登録 / device の可否を区別し、AAC は選択肢から省略する。
事前確認は範囲先頭の `render.explain`。全範囲・音声・codec の受理を保証しない文言を添え、
型付きエラーと未確認 / 古い確認がある間は投入できない。投入時の `expected_revision`
は捕捉した snapshot を検査 revision に固定し、競合は「再確認」の banner で扱う。
`render.submit` の固定 snapshot・独立 worker、`job.cancel` を再利用し、1秒以上の
間隔の一回の `job.list` で進捗 / 失敗を表示する。active job がないと停止し、ページ再入場と
明示した更新で再読込できる。JobRow の interrupted は自動再開しない中断状態を示す。
設定、選択、filter、比較入力は表示候補であり、編集可能な第二の作品状態ではない。

設計判断は [ADR-0078](../adr/0078-template-export-pages-and-inspected-snapshot.md)、
実行した checks と SwiftPM / Metal / 両 theme の pending 手順は [GUI-004 の検証](../testing/gui-004.md)。

## 未接続の編集操作の追跡

GUI-001〜004の完了は、すべての設計上のコントロールが編集可能という意味ではない。EditのEffects追加・速度/ソース開始/逆再生・合成設定・トラック表示/ミュート、Motionのガイド/スナップ、色・書体/ウェイト・複数Text style spanなど、監査時に無効化または表示専用だった操作をGUI-007（M5）で追跡する。M5作業ツリーでは一部のUIと共有APIを追加中であり、実装済みの操作と未完の受け入れを区別する。根拠とAPI/UIの区別は [現在の実装範囲と残件](../roadmap/implementation-status.md#macos-gui-で残る明示的な制限) を参照する。
