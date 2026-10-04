# LayerRow

Composition の階層（SceneNode の木）の 1 行で、containment parent による字下げと、別に持つ transform parent を併記する。

## 利用側が渡すもの

- 種類: `Group` | `Null` | `Shape` | `Text` | `Media` | `CompositionInstance`。
- 名前、深さ（containment parent をたどった段数）、子の有無と開閉状態。
- 任意: transform parent の名前（containment parent と異なるときだけ）。
- 状態: 選択、非表示、ロック。各トグルは Command を 1 回発行する。選択と開閉は UI 状態で、作品に書き込まない。

## 見た目

- 高さ `row-height`、字下げは 1 段ごとに `space-3`。列は開閉（12px）/ 種類アイコン（14px）/ 名前 / トグル。
- アイコン: Group `folder`、Null `crosshair`、Shape `shapes`、Text `type` は `ink-muted`。Media は素材の `kind-*`（映像なら `kind-video`）、CompositionInstance は `kind-composition`。
- transform parent は名前の後ろに `link` アイコン + 親の名前を `caption` の `ink-muted` で出す。
- トグル（表示・ロック）は既定の状態なら hover 時だけ出し、既定から外れたもの（非表示・ロック中）は常に出す。
- 非表示: 名前とアイコンを `ink-muted`、トグルは `eye-off`。ロック: 名前を斜体、トグルは `lock`。選択: `selection-bg`。

## 使い分け

- する: 字下げは containment parent だけで決める。transform parent を字下げで表さない。
- しない: 配列の順番や表示名を ID として扱わない（並べ替え・改名しても選択は保たれる）。
- しない: 外部変更で選択中のレイヤーが消えたときに黙って別の行を選ばない（扱いは GUI-001 で決める）。
