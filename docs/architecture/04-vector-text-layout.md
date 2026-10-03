# 04 ベクター・日本語テキスト・レイアウト

## ベクター

Path、Fill、Stroke、Gradient、ClipPath を意味的 IR に保持する。最初からビットマップ化して保存しない。

| 用途 | 候補 |
|---|---|
| 幾何演算 | kurbo |
| テッセレーション | lyon |
| SVG 読み込み | usvg |

これらの完全な SVG 互換やあらゆる Path 演算を前提にしない。
SVG 読み込みは対応表を持つ。外部 URL、script、外部フォント等は自動取得・実行せず、明示インポートする。

初期から矩形、角丸矩形、楕円、ベジェパス、単色塗り・線・基本グラデーションを扱う。
Trim path、線端・破線のアニメーション、Path boolean、morph は段階実装する。

### VEC-001 の実装規約

`kronello-model` の `Shape` / `ShapeGeometry` / `Fill` / `Stroke` を `Project.shapes` に保存し、`NodeKind::Shape.content_ref` の `ContentId` で参照する。矩形（共通の角丸半径）、楕円、複数 subpath の Move / Line / Quad / Cubic / Close を保持する。矩形・楕円はローカル原点 `(0, 0)` から size の矩形内に収める。角丸半径は非負値を正本に保持し、導出時だけ短辺の半分を上限にする。

- size・corner_radius・Path・fill/stroke の Color・stroke_width・join・cap・miter_limit は描画ノードの既存 `Property` を `PropertyId` で参照する。`shape_descriptors()` を `SchemaRegistry::with_builtin()` に追加登録する。fill は `kronello.fill_color`、幅は `kronello.stroke_width`、独立した線色は `kronello.shape.stroke_color` を用いる。join / cap と Path の現段階の補間は Hold のみ。miter_limit は無次元で 1 以上、size・半径・幅は非負の `design_px`。
- fill は単色と Nonzero / Evenodd を保持する。色は既存 `Color` のタグ付き straight RGB と独立 alpha を再利用する。グラデーション、ClipPath、破線、stroke tessellation、Path morph は今回未実装。
- `validate_shape_contents` で参照先・ノード内 Property の型・単位・ローカル座標を照合する。`Shape::resolve` には任意時刻・instance の評価後の値を渡し、負寸法・半径・幅、不正な enum、miter_limit、不正 Path 順序を型付きエラーにする。非有限値は既存 `FiniteF64` が拒否する。Property descriptor の範囲は Modifier 適用後に評価層でも検証する。
- `Project.shapes` は省略可能な追加フィールドとし、空集合は出力しない。旧 schema_version 1 の Shape を持たない文書は同じ値で往復する。未知フィールド・形状 variant は既存 `DocumentObject::Opaque` に保持し、編集・実行可能とは扱わない。公開 JSON Schema は共通 Rust 型から再生成する。
- 純粋な `kronello-vector::flatten` は kurbo 0.13.1（MIT OR Apache-2.0）で評価値からローカル `design_px` の polyline を導出する。`FlattenRequest` の出力 scale と画素 tolerance は保存しない。高 scale では設計単位の tolerance を小さくする。kurbo の近似 tolerance は厳密な誤差保証ではない。極端な座標・精度・命令数は保守的な計算予算エラーで拒否する。アスペクト比変更は自動で再レイアウトしない。

### 設計寸法と出力解像度

Composition の design_extent と出力画素数を分ける。同じ 16:9 で解像度だけを変える場合は原則再レイアウトしない。
16:9 → 9:16 等のアスペクト比変更は、明示した responsive variant / constraint で再レイアウトする。

設計寸法・Path・線幅・フォントサイズ・bounds は `design_px` を用い、Composition の左上原点、+X は右、+Y は下とする。ノードのローカル座標から親空間への変換と、出力画素への写像を分ける。外部 SVG 等の単位は import 境界で変換する（[ADR-0043](../adr/0043-semantic-dependencies-and-units.md)「単位と座標」）。

## 日本語テキスト

```text
UTF-8 + style spans + ruby associations
 -> grapheme / shaping clusters
 -> glyph selection, metrics, Japanese line breaking
 -> LayoutResult (lines, glyphs, bounds, anchors)
 -> animation units / selectors
 -> glyph coverage / paths
 -> linear-HDR compositing
```

Parley / Fontique を基礎候補とするが、禁則、ルビ、縦書きの要件充足は個別に検証・補完する。

- Unicode の書記素クラスタとグリフは一対一ではない。元テキスト範囲から組版クラスタ・グリフへの対応を保持する。
- 「一文字ずつ」は既定で組版クラスタを壊さない AnimationUnit に変換する。
- ルビ付き文字は親文字 + ルビを一体の単位にする既定動作を用意し、独立演出は明示指定とする。
- セレクターは文字、行、語、範囲、タグに対応するが、語分割の辞書・アルゴリズムを版管理する。
- テキスト更新で範囲指定が無効になった場合は再計算を報告し、曖昧な古いグリフ番号をそのまま使用しない。

## レイアウトと描画の分離

Position / Opacity の変更では原則組版を再実行しない。本文、フォント、サイズ、折り返し幅の変更は組版キャッシュを無効化する。
語数・文字数が変わる型送りでは、原則として全文を組版してから表示単位を隠す。行が毎フレーム組み直される方式を既定にしない。

### bounds の段階

| bounds | 意味 |
|---|---|
| layout_bounds | レイアウト用の幅・高さ・行ボックス |
| ink_bounds | 実際の字形・線などが占める領域 |
| visual_bounds | shadow / glow / blur / transform を含む描画領域 |

背景帯の自動追従は layout_bounds 等の明示段階を参照する。
`背景幅 <- テキスト幅 + 余白` と `テキスト折り返し幅 <- 背景幅` のような循環は検出して拒否する。
解像度別の最大行数、最小文字サイズ、安全領域、overflow の方針をテンプレートの制約として持つ。

### 実装段階

- M2: テキストの layout_bounds への単方向参照による背景帯追従と、`max_lines` 超過の overflow 検出（TEMPLATE-001）。
- M3: 3 種の bounds の区別、循環診断、responsive variant による再レイアウト（LAYOUT-001、TEMPLATE-002）。

## 描画バックエンドの制約

確認した Vello の `render_to_texture` API は `Rgba8Unorm` を要求する。この経路を HDR 全画面合成へ直結しない（[ADR-0012](../adr/0012-hdr-compositing-vs-vector-rasterizer.md)）。

- 文字や単色形状の coverage マスクを生成して、RGBA16F 側で色を適用する構成を優先する。
- 任意のグラデーションや色付き SVG まで「coverage だけで完全再現できる」とは扱わず、必要な色処理は独自 GPU 描画パスで実装する。
- 同じベクター IR から異なるラスタライザーへ渡せるようにし、Vello を作品の保存形式にしない。

coverage は `[0, 1]` の無次元値で、色の伝達関数を適用しない。色付き raster のアダプターは色空間と alpha 表現を明示し、作業用線形空間の premultiplied 画像へ変換する（[ADR-0044](../adr/0044-color-and-alpha-contracts.md)）。
