# 画面

状態: M3 の GUI 実装前に策定（2026-10-04）。ネイティブ実装による検証は未了。寸法は 1440×900 のウインドウでの値。

メインウインドウの画面仕様。部品の見た目は [components/](../components/)、値は [tokens.json](../tokens.json) を参照し、ここでは配置・寸法・状態の扱いを定める。構成の決定と理由は [ADR-0055](../../adr/0055-main-window-pages-and-workspaces.md)。

| 文書 | 内容 |
|---|---|
| [ウインドウの骨格](window.md) | 最小サイズ、ツールバー、ページ、ワークスペース、ステータスバー、テーマ |
| [モーション](motion.md) | Composition の編集: Layers、Viewer、ToolStrip、Inspector、Dope sheet / Curve editor |
| [編集](edit.md) | Sequence の編集: Project、Viewer、クリップの Inspector、トラック |
| [テンプレート](template.md) | variant の比較、公開入力、版、尺のポリシー |
| [書き出し](export.md) | 書き出し設定、書き出し前の確認、ジョブ |
| [エラーと競合の状態](states.md) | Undo の競合、外部変更、描画と評価のエラー、安全モード、素材の欠落 |

## 共通の規則

- 各ページは四方型（左・中央・右・下）を基本にし、ページごとに列の幅と下段の高さを変える。
- パネルの配置・大きさ・タブ・ToolStrip の置き場所・Dope sheet の値の列の開閉はワークスペースの UI 状態であり、ユーザーごとの状態領域に保存する（[ADR-0033](../../adr/0033-ui-state-in-user-state-area.md)）。`.kronello` には書き込まない。
- 画面に出す時刻はタイムコードか秒 + フレームで、浮動小数点の秒を出さない。
- 型付きエラーは色・アイコン・エラーコード・文言の組で示し、最終出力を黙って続行しない。
- Light テーマは同じ配置のままトークンを切り替える。画面ごとの Light 専用の配置は作らない。
