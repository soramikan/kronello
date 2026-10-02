# ADR-0002: Timeline と Composition は別モデルとし、共通 IR へ変換する

- 状態: 採用（v0.2 仕様から継承。実装による検証は未了）
- 日付: 2026-10-01

## 背景

カット編集はクリップの時間区間・リップル・リンクを扱い、モーショングラフィックスは空間階層・親子付け・プロパティを扱う。一つのモデルに混ぜると双方の不変条件が曖昧になる。

## 決定

- Timeline（placement model）と Composition（scene model）を別の編集モデルにする。
- 両者を同じ Scene IR / Render DAG へコンパイルする。
- Composition は SourceRef として Timeline へ配置できる。Composition から別の Composition を参照できるが、参照循環は禁止する。

## 影響

- 時間・Property・組版・合成・レンダーの基盤は共有される。
- Document Compiler が二つのモデルを一つの IR へまとめる責務を持つ。

## 関連

- [00 概要](../architecture/00-overview.md)
- [01 データモデル](../architecture/01-data-model.md)
