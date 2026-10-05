# ADR-0066: グラデーションの座標・spread・補間空間を明示する

- 状態: 採用（CPU / 静的検証と Metal 実機検証は区別する）
- 日付: 2026-10-05
- 対象: VEC-004

## 背景

VEC-003 はローカル design_px、linear / radial、pad、作業用線形 premultiplied 補間を実装した。
VEC-004 は同じ coverage と独立した paint 経路を拡張する。
ADR-0012 / 0024 / 0043 / 0044 の線形合成・単位・alpha 規約を維持し、Color Property の補間を変更しない。

## 決定

### 保存と互換性

各 `Gradient` に `options: GradientOptions` を持たせる。
`spread` は `pad` / `repeat` / `reflect`、`units` は `local_design` / `object_bounding_box`。
`interpolation` は下表、`interpolation_version` は **1**。
`transform` は 2×3 affine 行列（gradient → units）。省略した options / 欄は
VEC-003 と同じ pad、working_linear_premultiplied、版 1、local_design、単位行列に正規化する。
未知欄・enum は既存 `DocumentObject::Opaque` に保持して実行を拒否する。
未知補間版は `ShapeError::UnsupportedGradientVersion`、最終レンダーは `UNSUPPORTED_FEATURE`。
不正な寸法・stop・特異行列は型付き検証エラーとし、並べ替え・clamp・近似で修正しない。

| interpolation | stop の準備 | 補間後の処理 |
|---|---|---|
| working_linear_premultiplied | 作業用線形空間へ変換して premultiply | そのまま内部 paint |
| working_linear_straight | 作業用線形 straight RGB と alpha | 補間した alpha で premultiply |
| srgb_straight | Rec.709 原色へ変換し、sRGB encode した straight RGB と alpha | sRGB decode → 作業原色変換 → premultiply |
| srgb_premultiplied | sRGB encoded RGB に alpha を掛ける | alpha が正なら除算 → decode → 作業原色変換 → premultiply。ゼロは全成分ゼロ |

補間空間は stop の入力タグと独立。straight モードでは透明 stop の保存 RGB も補間する。
alpha に伝達関数を適用せず、負 RGB / 1 超を中間で clamp しない。
内部補間の除算に外部境界の alpha epsilon を適用しない。
premultiplied モードの透明 stop は色変換前にゼロとし、straight モードも
補間結果の alpha がゼロなら paint をゼロにする。隠れた RGB の色変換 overflow を
`0 * infinity` で出力へ持ち込まず、CPU と WGSL のゼロ alpha 規約を揃える。
stop は既存の色・offset Property、2〜256 個、offset は `[0,1]` の非減少順。
同位置の最後の stop がその位置で勝つ。spread を適用してから stop を探索する。
snapshot / metadata / raster cache の全体意味版は `vec004-explicit-interpolation-v1`。
旧意味版の固定 RenderSnapshot は既存の版検査で拒否し、新規 snapshot を作り直す。

### geometry と spread

linear の未制限 parameter は軸への射影、radial は中心からの距離 / 半径。
repeat は `t - floor(t)`、reflect は `u = t - 2*floor(t/2)`、`u <= 1 ? u : 2-u`。
負 parameter にも同じ規約を使う。repeat の整数境界は 0、reflect の奇数境界は 1。

`focal_radial` は外円 `(center, radius)` と焦点円 `(focal, focal_radius)`。
焦点半径は非負、`distance(center,focal) + focal_radius < radius` を要求する。
焦点円内は parameter 0。外側は `C(t)=focal+t*(center-focal)`、
`R(t)=focal_radius+t*(radius-focal_radius)` の円上に点が乗る非負解を使う。
`q=p-focal, d=center-focal, dr=radius-focal_radius` とすると
`A=dr²-dot(d,d), B=dot(q,d)+focal_radius*dr, C=dot(q,q)-focal_radius²`。
`C>0` の解は `(-B+sqrt(B²+A*C))/A`。B が非負なら同値の
`C/(sqrt(B²+A*C)+B)` で桁落ちを避ける。接する円・交差円・外部焦点は拒否する。
float32 lowering 後にも包含と `A>0` を検証し、不正になった値を近似しない。
sampling 時の座標・焦点円の判別式・NaN parameter・周期 spread の無限 parameter は、
最後の stop 色へ置換せず `GpuError::InvalidInput` とする。
CPU は raster 面の検証へ失敗を伝播し、WGSL は共有 sticky validation flag を立てる。
旧 pad の無限 parameter は端点が一意に決まるため、VEC-003 と同じ端点色を維持する。

`conic` は `center`、度単位の `start_angle`、正で 360 以下の `sweep_angle`。
+X が角度 0、+Y に向かう時計回り。開始角からの角度を `[0,360)` に正規化して sweep で割る。
中心点は parameter 0、開始 ray が seam。部分 sweep の残りにも spread を適用する。
負 sweep / 360 超の sweep は未対応として入力エラーにする。

### 座標と text

ローカル座標では gradient transform を先に、node world / ROI 写像を後に適用する。
bbox 座標では `node * bbox * gradient_transform`。Shape の bbox は線幅や effect を含めない
解析的 geometry bounds（Bezier の極値も含む）。幅または高さ 0、空 bbox は拒否する。
fill と stroke は別の gradient transform を持てる。非一様な gradient transform は楕円状の radial 等を作れる。
非一様 node transform 下の stroke は VEC-005 の未対応境界を維持する。

`TextStyleSpan.gradient` は省略可能。stop は同じ node の Property を評価する。
paint は shaping 後に `style_index` から各 PositionedGlyph へ付け直す。
bbox は text 全体の positioned `ink_bounds` を使い、glyph ごとに gradient を再開しない。
本文・grapheme / shaping cluster・AnimationUnit・outline・配置は paint の変更に依存させない。
layout / geometry cache は paint を除外し、raster cache は全 options / stop / 逆写像を含む。

### SDR 8bit banding

8bit の長い滑らかな ramp は量子化の段差を持ち、対策が必要になる場合がある。
VEC-004 では **dither を採用しない**。現行の最終画像出力は RGBA16F / 16bit PNG で、
8bit SDR 最終出力の量子化 API をこのタスクで追加しない。
一方、native preview は `Bgra8Unorm` の表示境界を持つため、長い ramp の banding が起こり得る。
VEC-004 では preview を含めた自動 dither を採用せず、8bit 表示の視覚品質保証は追加しない。
coverage、線形合成、中間面へノイズを入れると、16bit 出力や mask / effect / golden にまで影響する。
将来の 8bit exporter / preview の表示境界で、sRGB encode 後・量子化直前に明示的な固定 seed と絶対画素座標を使う
決定的な dither を検討し、tile / ROI / 実行順に依存しないことを検証する。
現在の出力について 8bit banding の改善を保証したとは扱わない。

## 検証と残件

[VEC-004 の検証](../testing/vec-004.md) に受け入れ条件と実行結果を記録する。
CPU reference と WGSL は同じ float32 規約と 4×4 coverage を使う。
Metal pixel / golden は host run 待ち。比較誤差は ADR-0047 / QA-001 の既存規約を維持する。
既存 baseline を編集・暗黙更新せず、clean commit の新候補をレビューして明示採用する。
