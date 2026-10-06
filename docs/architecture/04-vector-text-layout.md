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
破線 offset のアニメーションは VEC-005 の明示 stroke options で扱う。
Trim path、線端のアニメーション、Path boolean、morph は段階実装する。

### VEC-001 の実装規約

`kronello-model` の `Shape` / `ShapeGeometry` / `Fill` / `Stroke` を `Project.shapes` に保存し、`NodeKind::Shape.content_ref` の `ContentId` で参照する。矩形（共通の角丸半径）、楕円、複数 subpath の Move / Line / Quad / Cubic / Close を保持する。矩形・楕円はローカル原点 `(0, 0)` から size の矩形内に収める。角丸半径は非負値を正本に保持し、導出時だけ短辺の半分を上限にする。

- size・corner_radius・Path・fill/stroke の Color・stroke_width・join・cap・miter_limit は描画ノードの既存 `Property` を `PropertyId` で参照する。`shape_descriptors()` を `SchemaRegistry::with_builtin()` に追加登録する。fill は `kronello.fill_color`、幅は `kronello.stroke_width`、独立した線色は `kronello.shape.stroke_color` を用いる。join / cap と Path の現段階の補間は Hold のみ。miter_limit は無次元で 1 以上、size・半径・幅は非負の `design_px`。
- fill は単色と Nonzero / Evenodd を保持する。色は既存 `Color` のタグ付き straight RGB と独立 alpha を再利用する。M1のVEC-001当初はグラデーション、ClipPath、破線、stroke tessellation、Path morphを含めなかった。現行のfill/stroke・ClipPathは後続節とVEC-003〜005の検証記録を参照し、Trim Path / morph / SVGはVEC-002の未実装範囲とする。
- `validate_shape_contents` で参照先・ノード内 Property の型・単位・ローカル座標を照合する。`Shape::resolve` には任意時刻・instance の評価後の値を渡し、負寸法・半径・幅、不正な enum、miter_limit、不正 Path 順序を型付きエラーにする。非有限値は既存 `FiniteF64` が拒否する。Property descriptor の範囲は Modifier 適用後に評価層でも検証する。
- `Project.shapes` は省略可能な追加フィールドとし、空集合は出力しない。旧 schema_version 1 の Shape を持たない文書は同じ値で往復する。未知フィールド・形状 variant は既存 `DocumentObject::Opaque` に保持し、編集・実行可能とは扱わない。公開 JSON Schema は共通 Rust 型から再生成する。
- 純粋な `kronello-vector::flatten` は kurbo 0.13.1（MIT OR Apache-2.0）で評価値からローカル `design_px` の polyline を導出する。`FlattenRequest` の出力 scale と画素 tolerance は保存しない。高 scale では設計単位の tolerance を小さくする。kurbo の近似 tolerance は厳密な誤差保証ではない。極端な座標・精度・命令数は保守的な計算予算エラーで拒否する。アスペクト比変更は自動で再レイアウトしない。

### 線とグラデーションの実装範囲

M1 の VEC-003 と M3 の VEC-004 / VEC-005 の範囲、後続タスクの境界を固定する。未対応の機能を含む文書は保存時に失わないが、最終レンダーは `UNSUPPORTED_FEATURE` で拒否する（[ADR-0010](../adr/0010-unsupported-features-fail-final-render.md)）。

| 項目 | M1（VEC-003） | M3（VEC-004 / VEC-005）/ 後続 |
|---|---|---|
| 線の join / cap | miter（miter limit 既定 4）/ bevel / round、butt / square / round | — |
| 破線 | なし | 明示 options の dash 配列・offset Property（VEC-005、Metal 検証待ち） |
| 線の位置 | 中央のみ | center / inside / outside、closed contour の fill-rule clip（VEC-005） |
| 非一様 scale / skew 下の線幅 | `UNSUPPORTED_FEATURE` | version 2 はローカル線を affine で写す（VEC-005） |
| グラデーションの種類 | 線形、放射（中心と半径） | 焦点付き放射（焦点位置・焦点半径）、円錐 / sweep（VEC-004、Metal 検証待ち） |
| 範囲外の扱い（spread） | pad のみ | repeat / reflect（VEC-004） |
| 色の補間空間 | 作業用線形空間の premultiplied に固定 | グラデーションごとの明示指定（sRGB・straight 等）と補間空間の意味の版（VEC-004） |
| 座標系 | 図形のローカル `design_px` | bounding box 基準の座標、gradient transform（VEC-004） |
| 適用先 | Shape の fill / stroke | Text の fill（組版クラスタを壊さない、VEC-004） |
| stop のアニメーション | 色と位置 | — |
| SDR 8bit 出力の banding | 対策なし | VEC-004 では dither を採用しない。8bit exporter の量子化境界で再検討（ADR-0066） |
| Trim path、morph、SVG 対応表 | なし | VEC-002（M5） |

### VEC-003 の実装規約

- 既存の join / cap / miter_limit Property（既定 miter / butt / 4）をそのまま評価する。中央線の各線分を幅の矩形へ展開し、外側接合を三角形で埋める。miter は offset 線の交点まで延長し、交点の中心からの距離 / 半幅が limit を越えれば bevel とする。round join / cap は半幅の円、square cap は端を半幅延長した矩形、butt cap は端で終了する。閉 contour に cap を付けず、連続同一点は方向のない線分として除去する。全点同一の開 contour は round cap の円だけを持ち、その他は無被覆。幅 0 は無被覆。三角形の辺・円周は含み、面積 0 の三角形は無被覆。平行な接合（単位方向の cross = 0）は追加三角形を作らない。
- `Fill.gradient` / `Stroke.gradient` は省略可能な `Gradient`。指定すると従来の `color` に代わって paint を与える（従来の color Property も参照・型検証を維持する）。`Linear { start, end, stops }` と `Radial { center, radius, stops }` はローカル `design_px`、線形の両端は異なり、放射の半径は正。spread は常に pad。ノードと出力 region の affine 写像の逆でサンプル位置をローカル座標へ戻す。この導出写像は保存された gradient transform 機能ではない。逆写像が存在しない gradient は型付き入力エラーにする。
- `GradientStop` の color / offset はノード所有の `PropertyId`。再利用可能な `kronello.shape.gradient_color` / `kronello.shape.gradient_offset` descriptor を登録する。stop descriptor は一つのノードに複数配置でき、評価は PropertyId ごとに行う。transform / opacity 等の singleton descriptor 重複は引き続き拒否する。offset は有限の `[0,1]`、stop 数は 2〜256、保存順は offset の非減少順。評価後にも検証し、アニメーションで順序が逆転したら `ShapeError::InvalidGradient` とする。並べ替え・clamp で修正しない。同位置ではその位置の最後の stop が勝つ右連続の段差とし、直前の区間は最初の同位置 stop に向かう。
- stop の色を個別に sRGB decode / 原色変換 → 作業用線形空間 → premultiply し、その値を補間する。各 4×4 AA サンプルで paint を評価・被覆に応じて蓄積し、fill / stroke を別々に平均して stroke を fill へ source-over する。内部補間を unpremultiply しない。透明 stop の RGB を持ち込まず、HDR の負値・1 超も clamp しない。
- CPU / GPU は同じ float32 の展開済み三角形・円を使う。curve の flatten は既存の画素 tolerance（既定 0.02 px）。snapshot の coverage は `vec003-grid4-v2`、stroke geometry は `vec003-centered-stroke-v1`、gradient interpolation は `vec003-linear-premultiplied-pad-v1`。vector flatten / color の既存意味版は変更しない。raster key は stop 値・gradient geometry・逆写像・線幅 / join / cap / limit・追加意味版を含む。paint 変更ではローカル輪郭の geometry cache を再利用する。
- options を伴わない未知の stroke 拡張と未知の gradient フィールド・variant は strict な既知 Shape / Text として解釈せず、`DocumentObject::Opaque` で値と所属を保持する。選択出力が必要とする opaque content は最終レンダーで `UNSUPPORTED_FEATURE`。旧 stroke 幾何版の非一様変換の線も同じエラー。機能を既定値へ置換しない。

受け入れ条件のテスト対応と検証範囲は [VEC-003 の検証](../testing/vec-003.md) を参照。

### VEC-004 の実装規約

[ADR-0066](../adr/0066-explicit-gradient-semantics.md) で gradient ごとの `GradientOptions`
（spread / interpolation / interpolation_version / units / transform）を固定する。
省略値は VEC-003 の pad / working_linear_premultiplied / version 1 / local_design / 単位行列。
repeat / reflect は負 parameter にも floor の周期規約を適用する。
焦点円を外円の内部に厳密に含む `focal_radial`、時計回りの `conic`（正の sweep、360 度以下）を追加する。
補間は working_linear_premultiplied / working_linear_straight / srgb_straight / srgb_premultiplied。
透明 stop の RGB は straight モードでは保存・補間し、画像 paint への変換時に premultiply する。

bbox は Shape の unstroked 解析的 geometry bounds、Text 全体の positioned ink_bounds。
`node * bbox * gradient_transform` の順に写し、fill / stroke は独立した transform を持つ。
空 / 退化 bbox、特異行列、不正な円・sweep・stop は型付きエラー。
`TextStyleSpan.gradient` の stop も node の Property を評価し、shaping 後に style_index で glyph に付ける。
paint の変更で cluster / AnimationUnit / outline / layout key を変えず、raster key だけへ全 paint 入力を含める。
意味版は `vec004-explicit-interpolation-v1`、未知の補間版は最終レンダーで `UNSUPPORTED_FEATURE`。
SDR dither は採用せず、現行の RGBA16F / 16bit PNG にノイズを追加しない。
CPU / 静的検証と Metal / golden の残件は [VEC-004 の検証](../testing/vec-004.md) を参照。

### VEC-005 の実装規約

[ADR-0073](../adr/0073-local-stroke-extensions.md) に従い `Stroke.options` を明示選択する。
省略した作品は `vec003-centered-stroke-v1` の演算順・非一様変換拒否を維持する。
新 `vec005-local-stroke-v2` は局所の線幅・cap / join を affine で写す。
破線は flattened path のローカル弧長、奇数配列は複製、負 offset は周期 wrap、閉 contour の seam は結合する。
zero dash は butt 無被覆 / round 円 / square 局所軸の正方形。非空全 zero・負長・非有限長は拒否する。
16,384 subdivision steps / fragments の上限超過は `STROKE_BUDGET_EXCEEDED`。
`kronello.shape.dash_offset` は Scalar / DesignPx の通常 animatable Property。
inside / outside は幅 2w の中央線と fill-rule interior / 補集合の交差。
開 contour は `STROKE_OPEN_ALIGNMENT`。layout envelope は維持し、ink / visual と pixel ROI に affine support を反映する。
共通 `ShapeSet` / Property / Curve 編集、snapshot の対応版、raster cache と golden manifest に入力を記録する。
[VEC-005 検証](../testing/vec-005.md) に CPU 証拠と pending host run を分けて記録する。

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

TEXT-001 の横書きは下記の決定的な構成で実装した。Parley / Fontique は、将来の高度な組版で比較する候補として残す。禁則、ルビ、縦書きの要件充足は個別に検証・補完する。

- Unicode の書記素クラスタとグリフは一対一ではない。元テキスト範囲から組版クラスタ・グリフへの対応を保持する。
- 「一文字ずつ」は既定で組版クラスタを壊さない AnimationUnit に変換する。
- ルビ付き文字は親文字 + ルビを一体の単位にする既定動作を用意し、独立演出は明示指定とする。
- セレクターは文字、行、語、範囲、タグに対応するが、語分割の辞書・アルゴリズムを版管理する。
- テキスト更新で範囲指定が無効になった場合は再計算を報告し、曖昧な古いグリフ番号をそのまま使用しない。

### TEXT-001 の実装規約（組版意味版 1）

`kronello-model::TextDocument` は正規化しない UTF-8 本文、半開 byte range の `TextStyleSpan`、`FontRef`、横書き / 縦書きの方向、ルビ関連、`layout_version` を持つ。`Project.texts` に保存し、`NodeKind::Text.content_ref` で参照する。本文と style の範囲更新は一体で行う。style は本文を隙間・重複なく覆い、拡張書記素の境界に限る。未知フィールド・方向 variant は `DocumentObject::Opaque` に保持し、未知組版版は `Project.ensure_editable` と組版入口で拒否する。旧 schema_version 1 文書では `texts` の省略を空集合として読み、空集合は出力しない。公開 Schema は共通 Rust 型から再生成する。

- size / fill はノード所有の `PropertyId`、wrap_width / line_height / alignment も同じ評価基盤の参照とする。`text_descriptors()` を `SchemaRegistry::with_builtin()` に追加登録する。font_size・wrap_width・line_height は正の `design_px`、fill は既存の `kronello.fill_color`、alignment は `start` / `center` / `end` の Hold enum。`validate_text_contents` は内容の参照・Property の型と単位・定数の制約を照合する。`TextDocument::resolve` は任意時刻・instance の最終評価値を受け、再検証して `ResolvedText` を生成する。本文・FontRef・style 範囲は今回 Property にしない。
- `FontRef` は family、PostScript 名、SHA-256（小文字 hex）、face index を固定する。純粋な `pin_font(bytes, face_index)` は import 境界用の identity を生成する。`layout(&ResolvedText, &[FontData])` は呼出側が渡した bytes の hash・face・metadata を照合し、システム探索や代替フォントを使わない。identity と一致する入力が欠ければ `MissingFont`、異なる bytes は `FontHashMismatch`、名前違いは `FontIdentityMismatch`。同じ identity の入力が複数あれば曖昧な選択を拒否する。
- ライブラリは rustybuzz **0.20.1（MIT）** の OpenType shaping、ttf-parser **0.25.1（MIT OR Apache-2.0）** の metrics / cmap / outline、unicode-segmentation **1.12.0（MIT OR Apache-2.0、Unicode 16）**、unicode-linebreak **0.1.5（Apache-2.0、Unicode 15）**、unicode-script **0.5.8（MIT OR Apache-2.0）**、sha2 **0.10.9（MIT OR Apache-2.0）**。前 5 者を exact dependency、全体を Cargo.lock で固定する。少数の純粋な部品で font bytes とクラスタを直接追跡できるためこの構成を選んだ。Parley / Fontique の自動フォント解決や高度な行組版は今回必要としない。[rustybuzz API](https://docs.rs/rustybuzz/0.20.1/rustybuzz/)、[ttf-parser API](https://docs.rs/ttf-parser/0.25.1/ttf_parser/) を参照。
- style / script / 明示改行ごとに language `ja`、左から右の shaping を行う。Unicode の拡張書記素内の各 codepoint に同じ元 cluster を割り当て、書記素を壊さない monotone cluster で、元 byte range ↔ 書記素 ↔ shaping cluster ↔ glyph の対応を保持する。既定 `AnimationUnit` は shaping cluster を分割せず、shaper の unsafe-to-break 境界も隣と一体化する。これらの index は導出値で、保存 ID や本文変更後の stable selector にしない。
- soft break は Unicode 改行候補、cluster 境界、shaper の安全境界、基本禁則の積集合から貪欲に選ぶ。行頭禁止は句読点・閉じ括弧・長音・小書きかな等、行末禁止は開き括弧等。LF / CR / CRLF / U+2028 / U+2029 は禁則より優先する明示改行として、glyph のない source cluster を保持する。空本文・末尾改行は空の行 box を持つ。空白を削除・圧縮せず、ぶら下げ・追い込みは行わない。分割不能な語や禁則区間が幅を超えた場合は `LayoutLine.overflow = true` を返し、clip や cluster の強制分割をしない。後続 template の overflow 方針で許可・拒否を決める。
- line_height は絶対 baseline 間隔。先頭 baseline は全文 style の最大 ascender、各行はそこから line_height ずつ下げる。行 box の高さも line_height とする。字形が box を越えることを許し、`layout_bounds` と分けて `ink_bounds` で表す。`layout_bounds` は `(0, 0)` から wrap_width × 行数 × line_height の矩形、`ink_bounds` は配置した outline の字形矩形の union（無 ink は `None`）。glyph の `position` と `Path` は text-local の `design_px`、+Y は下。Path は配置済みで、再度 position を足さない。fill はタグ付き straight Color のまま保持し、coverage raster と色付き合成は GPU-002 / RENDER-001 へ渡す。
- `.notdef` が出た cluster は元の文字列・range・FontRef を列挙する `MissingGlyphs`。IVS / variation selector は cmap の対応も必須とし、shaper に selector が黙って捨てられる場合も拒否する。固定 Noto fixture の ZWJ emoji は欠落エラー、IVS は指定字形を選ぶ。bitmap / SVG / color glyph 表現は `UnsupportedGlyphOutline`。縦書き・ルビ・可変フォント軸・RTL / bidi・tab / control は型付き未対応とし、通常の横書きへ置換しない。
- 意味版は `TextDocument.layout_version`、`TEXT_LAYOUT_VERSION` / `kronello-text::LAYOUT_VERSION` とも **1**。shaping・Unicode 分割・禁則・metrics・配置規約を変えると版を上げる。RenderSnapshot との対応付けは RENDER-001 の compile 境界で行う。関数は text IR の最終値・明示 bytes だけで結果を生成し、出力解像度・OS・呼出履歴を入力にしない。永続 cache は CACHE-001。本文 65,536 byte、style 4,096、glyph 131,072、導出 outline 合計 1,048,576 segment の保守的上限と非有限 geometry 検査を設ける。

受け入れ条件と再現手順は [TEXT-001 の検証](../testing/text-001.md) に記録する。ルビ・縦書きの実行、単語辞書、高度な selector は TEXT-002 の未実装範囲として残す。

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
- M3: LAYOUT-001 で 3 種の bounds、明示した帯の stage 選択、静的循環と幅 overflow の診断を実装。responsive variant による再レイアウトは TEMPLATE-002 の後続範囲。

## 描画バックエンドの制約

確認した Vello の `render_to_texture` API は `Rgba8Unorm` を要求する。この経路を HDR 全画面合成へ直結しない（[ADR-0012](../adr/0012-hdr-compositing-vs-vector-rasterizer.md)）。

- 文字や単色形状の coverage マスクを生成して、RGBA16F 側で色を適用する構成を優先する。
- 任意のグラデーションや色付き SVG まで「coverage だけで完全再現できる」とは扱わず、必要な色処理は独自 GPU 描画パスで実装する。
- 同じベクター IR から異なるラスタライザーへ渡せるようにし、Vello を作品の保存形式にしない。

coverage は `[0, 1]` の無次元値で、色の伝達関数を適用しない。色付き raster のアダプターは色空間と alpha 表現を明示し、作業用線形空間の premultiplied 画像へ変換する（[ADR-0044](../adr/0044-color-and-alpha-contracts.md)）。

### TEMPLATE-001 の背景帯と overflow（M2 実装）

背景帯は同一親空間の Rectangle とし、size / position へ text の確定 `layout_bounds` と design_px の padding を束縛する。
組版は render compiler が実行し、`RuntimePropertyKey::LayoutValue` と `DependencyDeclarations` を介して `kronello-eval` へ意味的値を渡す。
text / template の実装への evaluator の逆依存はない。
text の wrap_width を背景帯から読む循環は拒否する。公開 text 置換は水平・単一 style・ruby なしを対象とする。
max_lines 超過は node・実際の行数・最大行数を持つ `TemplateError::Overflow`、最終レンダーでは `TEMPLATE_OVERFLOW`。
検証手順は [TEMPLATE-001](../testing/template-001.md) を参照。


### LAYOUT-001 の実装規約（M3）

[ADR-0057](../adr/0057-layout-bounds-stages.md) により、`kronello-render::LayoutValue` は
`layout_bounds` / `ink_bounds` / `visual_bounds` を同じ座標空間の `DesignBounds {min, max}` または `None` として保持する。
Scene IR と `scene.query` の `evaluated.bounds` は、active node ごとの三段階を root Composition の `design_px` で同時に返す。
既存の `evaluated.layout_bounds` は text-local のまま。非アクティブな node に evaluated を付けない。

text の layout は wrap_width × 行数 × line_height、ink は組版 outline の union。
Shape は解析した幾何 envelope と中央線 support を区別し、一般の Bezier 線は miter / cap の保守的な包含矩形を使う。
各段階を world transform で写し、visual に renderer と同じ変換規約の blur / shadow を順に加える。
Group / Null / placement は子の三段階を union し、自身の effect は visual だけへ適用する。
mask、穴、透明 paint / opacity による tight な alpha 被覆の縮小は行わない。
semantic な blur support は連続の `3 * sigma`、pixel DAG は出力格子へ外向きに丸めるので、その丸めを保存値や再組版に混ぜない。

`TemplateBandBinding.bounds` は `layout`（既定・省略可）/ `ink` / `visual`。
layout / ink は text の bounds を共通親空間へ写し、visual は Composition 空間の AABB を親へ逆変換する。
親の特異変換は `LAYOUT_SINGULAR_TRANSFORM`。空 ink / visual は text 原点に padding だけの帯となる。
`DependencyDeclarations` に text Property → `RuntimePropertyKey::LayoutValue` → band Property を静的宣言し、visual では変換親・placement の Property も含める。
`DependencyGraph::dependency_order` で projection を供給し、帯の定義順へ依存させない。
逆向きの text wrap → band size の宣言は閉経路付き `PROPERTY_DEPENDENCY_CYCLE`。
公開 API に任意式や逆依存の authoring 入口を追加したものではなく、依存宣言は後続 compiler の接続点である。

`LayoutLine.overflow` の幅超過を compiler が `LAYOUT_OVERFLOW` とし、template 以外の text も最終出力を拒否する。
既存 max_lines の `TEMPLATE_OVERFLOW` と診断は維持する。幅超過は node / instance_path / line / advance / wrap_width を返す。
`SemanticVersions.bounds = 1` を固定し、未知版を拒否する。
正の Gaussian の非一様変換と glow は既存の未対応境界を維持する。
検証範囲と host の残件は [LAYOUT-001 検証](../testing/layout-001.md) を参照。

帯の対象 text は leaf node に限る。子の合成結果を組版時の字形 bounds へ混ぜず、子を持つ対象は `UNSUPPORTED_FEATURE` で拒否する。帯の対象でない text の子は通常の scene 合成と bounds 集約で扱う。
