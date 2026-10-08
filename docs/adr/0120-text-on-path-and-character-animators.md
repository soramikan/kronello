# ADR-0120: テキストオンパスと文字単位アニメーター拡張

- 状態: 採用
- 日付: 2026-10-08

## 背景

VEC-006 はテキストオンパス（ベジェパス上の文字配置）と
文字単位アニメーター（範囲・ランダム・追従）を要求する。
`TextDocument.character_animations` は TEXT-002 で offset /
opacity のみ実装済みであり、パス参照の表現とアニメーター
拡張の形を決める必要がある。

## 決定

### テキストオンパス

- `TextDocument` に `path: Option<PropertyId>` を追加する
  （`#[serde(default, skip_serializing_if = "Option::is_none")]`）。
  `ValueType::Path` のプロパティを参照し、ベジェパスに沿って
  baseline を配置する。
- レイアウト規約: 文字はパスの弧長に沿って配置し、各文字を
  接線方向に回転する。`alignment` はパス始点からの
  start / center / end に対応する。
- パス長を超えた文字は描画しない（切り捨て）。切り捨ては
  エラーではなく inspect で報告可能な状態として扱う。
- direction が vertical の場合や ruby との併用は M8 では
  `UNSUPPORTED_FEATURE` として型付き拒否し、horizontal +
  非 ruby のみを対象とする。

### 文字アニメーター拡張

- `CharacterAnimation` に以下を追加する:
  - `scale: Option<PropertyId>`（文字ごとの拡縮、1.0 中立）
  - `rotation: Option<PropertyId>`（文字ごとの回転、度）
  - `fill: Option<PropertyId>`（色オーバーライド）
  - `mode: AnimatorMode`（`step`/`ramp`/`follow`/`random`。
    既定は step）
  - `seed: Option<u32>`（random の固定シード。乱数は
    seeded PRNG で完全に決定的）
  - `follow_smoothing: PropertyId` or `delay`（follow 時の
    追従量）
- 既存の offset/opacity を維持し、追加フィールドはすべて
  Option + serde default で後方互換とする。

## 影響

- レイアウトは引き続き純粋関数であり、`path` 評価も
  固定時刻で行う。フォントロック・glyph hash 契約は変わらない。
- GUI はパス参照の選択とアニメーターの mode/seed 編集を
  Inspector に追加する。

## 関連

- VEC-006、TEXT-002、ADR-0105（式構文）、
  `crates/kronello-model/src/text.rs`
