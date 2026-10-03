# kronello-text

TEXT-001 の純粋な横書き日本語組版。入力は `kronello-model::ResolvedText` と `FontData`（明示した固定フォントの bytes）。システムフォント探索、ネットワーク、保存、GPU、永続 cache は持たない。

1. import 境界で `pin_font(bytes, face_index)` を呼び、FontRef を文書の style に保存する。
2. `text_descriptors()` を SchemaRegistry に登録し、`validate_text_contents` でノード内参照を検証する。
3. instance / time の Property 評価値を `TextDocument::resolve` に渡す。
4. `layout(&resolved, &[FontData { identity: &font_ref, bytes }])` を呼ぶ。必要フォントの SHA-256・face・名前を再検証する。

`LayoutResult` は行・配置 glyph・書記素と shaping cluster の対応・既定 AnimationUnit・layout/ink bounds を返す。glyph outline は配置済みの text-local `Path`、座標は `design_px`。色は tagged straight Color を維持し、後続の coverage raster / 線形合成へ渡す。

欠落フォント・hash/名前不一致・欠落 glyph は `LayoutError`。IVS が cmap にない場合もエラーにする。分割不能な区間は `LayoutLine.overflow` で示す。縦書き、ルビ、bidi、tab、可変フォント軸は今回未対応。`LAYOUT_VERSION = 1`。

契約・ライセンス・禁則・予算は [設計](../../docs/architecture/04-vector-text-layout.md)、検証は [TEXT-001](../../docs/testing/text-001.md) を参照。
