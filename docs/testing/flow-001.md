# FLOW-001 検証: ショートカット設定・レイアウト・Undo 履歴パネル

状態: `m8-lane-g` 作業ツリーで実装・検証（2026-10-08）。対象は [ADR-0121](../adr/0121-workflow-ui-persistence.md)。

## 実装範囲

### ショートカットのカスタマイズ

- `WorkflowSettings`（`KronelloAppModel/Workflow.swift`）が action 名 → `KeyBinding` の辞書を `UserDefaults` の `kronello.shortcuts` に保持する。未設定 action は `ShortcutAction.defaultBinding` にフォールバックし、既定値への再割当は override を削除する。
- `ShortcutAction` は File / Edit / ページ切替 / transport（再生・フレームステップ・先頭末尾）・編集ツール・モーションツール等 37 action を網羅し、従来のキー割当をそのまま既定値に写した（⌘N / ⌘O / ⌘W / ⌘Z / ⇧⌘Z / ⌘1-4、Space / ←→ / Home / End / Esc、Edit ページの Delete・⌥Delete・M・⇧M・I・O・⌥X・↑↓・V B Y U N H、Motion の V H Z M E P T）。
- メニューの `keyboardShortcut`（`KronelloCommands`）、Edit ページのトラック `onKeyPress`（`SequenceTracks`）、Motion viewer の `onKeyPress`（`MotionViewer`）をすべて `WorkflowSettings.binding(for:)` 参照に切替え、`KeyBinding.matches(_ press:)`（キー同一 + ⌃⌥⇧⌘ の完全一致、文字キーは `press.characters` の lowercase 比較で Shift 付きも受理）で判定する。`.kronello` には書き込まない。
- 設定アプリ画面に「ショートカット」節を追加。「割当」押下後の次の keyDown を `NSEvent` ローカルモニタで捕捉して即時反映（Esc で中止、「デフォルトに戻す」で全消去）。同一 `UserDefaults` への書込みは起動中の全ウィンドウに即時反映され、再起動後も保持される。

### ワークスペースレイアウトの保存

- `PageLayout`（左右下パネルの開閉 + 幅・高さ）を page ごとに `UserDefaults` の `kronello.pageLayouts` に保存し、`PageLayout.standard(for:)` が既定寸法（motion: 248/304/344、edit: 280/296/312）を返す。
- Motion ページ（`MotionPage`）と Edit ページ（`KREditLayout` へ panel 引数追加）が `workflow.layout(for:)` を参照して描画する。設定画面の「ワークスペース」節で page 選択・パネル開閉 checkbox・幅の `KRNumberField` を編集でき、即時反映される。
- 作品 revision には一切影響しない。既存の per-project `WorkspaceState`（ツールストリップ位置・値列の開閉）は従来どおり `UIStateStore` に残す。

### Undo 履歴パネル

- `HistoryPanel`（WorkflowSupport.swift）が共有 `history.list` の読み取り専用表示。`EditorModel.loadHistory()`（既存 `historySince` 経路、since "0"）で全件取得し、`HistoryEntry` が label（このセッションで適用した操作の eventLabels、undo イベントは「取り消し」、他は先頭 mutation の operation 名）・rev・セッション所属・観測時刻を保持する。`history.list` に時計フィールドがないため、観測時刻はセッション内で最初に見た時刻の表示用メタとして扱う。
- 行ごとの「取り消す」は既存の共有 `edit.undo` を `event_id` 指定で呼ぶ `EditorModel.undoEvent`。新しい inverse event が積まれ、`undoState.didSelectiveUndo` で対象を undo スタックから外し inverse を redo に積む（Redo で対象変更が再適用される）。
- `historyPanel` は delta 再読み込みで消えないよう独立リストを保持し、`history` didSet で新規 event を上に merge、undo 系成功後に `refreshHistoryPanel` で `undone` フラグを再読みする。`undone` 済み行はボタンを出さず「取消済み」を表示。失敗は `UNDO_CONFLICT` など既存の型付きエラーマッピングに流す。

## 受け入れ条件と証拠

| 条件 | テスト / 手順 | 状態 |
|---|---|---|
| ショートカットのカスタマイズ | `testBindingFallsBackToDefaultAndCanonicalizesModifiers`（既定フォールバック・修飾子正規化）、`testBindingOverridesPersistAcrossInstancesAndResetToDefault`（同一 suite の別 instance = 再起動相当で保持、既定再割当で override 削除）、`testCorruptShortcutDataFallsBackToDefaults` | 合格 |
| ワークスペースレイアウトの保存 | `testPageLayoutPersistsPerPageAndResets`（page ごとの保存・再起動相当での復元・他 page は既定のまま・reset） | 合格 |
| Undo 履歴パネル | `testHistoryPanelLoadsNewestFirstAndLabelsEntries`（新しい順・label / undone / own 表示）、`testSelectiveUndoCallsSharedEditUndoAndRefreshesPanel`（`edit.undo` に `event_id` / `base_revision` / `session_id` / `idempotency_key` が渡り、panel の undone が更新、inverse が redo に積まれる）、`testSelectiveUndoFailureSurfacesTypedError`（UNDO_CONFLICT が undoConflict に流れる） | 合格 |

## 実行記録

- `python3 scripts/build_ffi.py`: exit 0（`Libraries/libkronello_ffi.dylib`・`kronello` を生成）。
- `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --package-path apps/macos --disable-sandbox`: exit 0。
- `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift test --package-path apps/macos --disable-sandbox`: exit 0、103 passed / 0 failed / 1 skipped（新規 FlowShortcutTests 4 件・FlowHistoryTests 3 件を含む）。

## 残件

- ショートカット割当の実画面操作（NSEvent モニタ経路の体感）とパネル開閉の実 GUI 確認はモデル層テスト + ビルドで担保し、CUA 目視は統合検証に委ねる。
- `history.list` の Event に表示名・時計がないため、他セッションの変更は先頭 mutation の operation 名で表示する。より人間可読なラベルは API 側の拡張（将来タスク）で補強する余地がある。
- Edit ページのツールストリップ表示は既存 `editTool` のまま（ショートカットと同じ action id を共有する変更は未実施）。
