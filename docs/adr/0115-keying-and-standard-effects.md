# ADR-0115: キーイング（chroma/luma）と標準エフェクト拡張

- 状態: 採用
- 日付: 2026-10-08

## 背景

FX-005 は chroma key / luma key とスピル抑制、FX-006 は
glow / sharpen / vignette / warp 等の標準エフェクトを要求する。
いずれも既存の versioned `Effect` / `ResolvedEffect` の枠組みに
追加するが、エフェクト ID・パラメータ集合・数値範囲を
確定する必要がある。

## 決定

### エフェクト一覧（すべて `version = 1`）

- `kronello.keying.chroma` — `key_color`(Color), `similarity`,
  `edge_shrink`, `edge_feather`, `spill`（0..=1、スピル抑制量）。
  キー距離は YCbCr の Cb/Cr 距離で評価し、スピル抑制は
  キー色寄りの彩度を抑制する。
- `kronello.keying.luma` — `key_luma`, `tolerance`, `edge_shrink`,
  `edge_feather`。輝度距離でアルファを決める。
- `kronello.glow` — `threshold`, `radius`(design_px),
  `intensity`。閾値超の輝度を抽出しガウスぼかしして加算する。
- `kronello.sharpen` — `amount`, `radius`(design_px)。
  unsharp mask。
- `kronello.vignette` — `amount`, `midpoint`(0..1), `feather`,
  `roundness`(0..1)。
- `kronello.corner_pin` — `top_left`..`bottom_right` の 4 つの
  Vec2（design_px）で四隅を移す逆写像ワープ。
  （warp の M8 採用形態として corner pin を選ぶ。

### 規約

- すべて premultiplied working space で評価し、alpha の扱いを
  各 effect で明示する（keying は alpha を書き換える、
  glow/sharpen/vignette/corner_pin は alpha を保持または
  合成意味に従う）。
- キーイングと corner_pin 以外は pointwise ではなく
  周辺参照を持つため `bounds`/`halo` を拡張する（glow/sharpen は
  radius、corner_pin は移動先の逆写像範囲）。
- パラメータの有限・範囲検証は `resolve` で型付き拒否。
  CPU/GPU で bit 一致は要求しないが、意味的一致（許容差内）
  を検証テストで示す。

## 影響

- Inspector の effect 追加 UI に新エフェクトが列挙され、
  パラメータ編集は既存の scalar/vec2/color ヘルパーを流用する。
- WGSL の追加は FXC 互換を意識する（switch・動的ベクトル
  インデックスを避ける）。

## 関連

- FX-005、FX-006、ADR-0108、ADR-0109、FX-003
