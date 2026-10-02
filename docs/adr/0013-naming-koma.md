# ADR-0013: 名称を koma に統一する

- 状態: 採用
- 日付: 2026-10-02

## 背景

v0.2 仕様は CLI と crate に仮称 `ved` を使っていた。リポジトリ名は `koma`。

## 決定

- プロダクト名は Koma。
- CLI の実行ファイルは `koma`、crate の接頭辞は `koma-`、プロジェクトファイルの拡張子は `.koma`。
- `ved` という表記は新しく書かない（`docs/archive/` の元仕様を除く）。

## 影響

- 公開前に crates.io の空き状況と既存の名称・商標との衝突を確認する必要がある（OQ-02）。

## 関連

- [11 ワークスペース](../architecture/11-workspace.md)
- [未決事項](../open-questions.md)
