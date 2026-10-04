# アイコン

Kronello の UI は [Lucide](https://lucide.dev/) のアイコンだけを使う（[ADR-0054](../adr/0054-gui-design-system.md)）。

- 版: `lucide-static` 1.51.0 を基準とする。アプリへは SVG を改変せずに同梱し、版を上げるときは使用中のアイコンの形が変わっていないか確認する。
- ライセンス: ISC。Feather 由来の一部のアイコンは MIT。配布物には Lucide の LICENSE 全文を同梱する。いずれも GPL ではない。

## 描き方

- 単色の線アイコン。インクは `currentColor` で、親要素の文字色を継がせて描く: 通常 `ink-muted`、hover / pressed で `ink`、エラーは `danger`、種類のアイコンは `kind-*`。
- 24px グリッド・stroke-width 2 のまま、ツールバーは 14px、行内・クリップ内は 12px に縮小して使う（線は約 1〜1.2px になる）。stroke-width を上書きしない。
## 用途の対応

| 用途 | アイコン |
|---|---|
| 表示 / 非表示 | `eye` / `eye-off` |
| ロック / 解除 | `lock` / `lock-open` |
| 音声 / ミュート | `volume-2` / `volume-x` |
| 種類: 映像・静止画 | `film` / `image`（`kind-video`） |
| 種類: 音声 | `audio-lines`（`kind-audio`） |
| 種類: Composition | `layers`（`kind-composition`） |
| 種類: 字幕 | `captions`（`kind-subtitle`） |
| 種類: Generator | `sparkles`（`kind-generator`） |
| 種類: 調整 | `sliders-horizontal`（`kind-adjustment`） |
| レイヤー: Group / Null / Shape / Text | `folder` / `crosshair` / `shapes` / `type` |
| transform parent | `link` |
| 型付きエラー / 情報 | `triangle-alert`（`danger`） / `info` |
| 追加 / 閉じる / メニュー / 検索 | `plus` / `x` / `ellipsis` / `search` |
| 開閉 | `chevron-right` / `chevron-down` |
| 選択肢を開く（PopupButton） | `chevrons-up-down` |
| チェック（Menu・PopupButton） | `check` |
| 再生操作 | `play` / `pause` / `skip-back` / `skip-forward` / `step-back` / `step-forward` / `repeat` |
| 編集 | `scissors` / `copy` / `clipboard-paste` / `trash-2` |
| 再読込・再リンク / 保存と revision | `refresh-cw` / `history` |
| ジョブ: 実行中 / 待機 / 完了 / 中止 | `loader-circle` / `clock` / `circle-check` / `circle-x` |
| プロジェクト / 新規 / 開く | `clapperboard` / `file-plus` / `folder-open` |

ここにないアイコンが必要になったら Lucide から選び、この表に追加する。他のアイコンセットや絵文字を混ぜない。
