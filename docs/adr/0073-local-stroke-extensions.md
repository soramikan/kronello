# ADR-0073: 破線・線位置・affine 線幅を明示したローカル幾何版で扱う

- 状態: 採用（CPU / 静的検証と Metal 実測は区別する）
- 日付: 2026-10-05
- 対象: VEC-005

## 保存と互換性

`Stroke.options: Option<Box<StrokeOptions>>` を追加する。省略時は従来の
`vec003-centered-stroke-v1` を選び、保存 JSON に options を追加しない。
中央線を出力座標へ写してから等方の線幅で展開する旧演算順、非一様変換の拒否、
join / cap / miter-limit は維持する。旧作品の自動移行はしない。

新機能は `options.geometry_version = "vec005-local-stroke-v2"` の明示選択。
options は `alignment`（center / inside / outside）、`fill_rule`（nonzero / evenodd）、
`dash_array`（有限長の配列）、`dash_offset`（PropertyId）を必須とする。
未知版は `ShapeError::UnsupportedStrokeVersion` / `UNSUPPORTED_FEATURE`。
未知欄・enum は既存の opaque 保存規約を使い、最終出力で拒否する。
新規 RenderSnapshot の `SemanticVersions.stroke_geometry` は対応上限として新しい文字列を固定する。
旧文字列を固定した snapshot も実行できるが、新 options の実行は拒否する。
実際の演算は authored options の有無で選ぶ。coverage / flatten / 色 / gradient の意味版は維持する。

## 破線

- 配列はローカル `design_px`、kurbo の出力解像度依存 tolerance で flatten した polyline の弧長。
  原点は各 contour の最初の MoveTo。偶数 index は描画、奇数 index は gap。
  空配列は実線。奇数要素数は配列を一回複製して偶数にする。
- offset は正なら先に pattern を消費し、`offset.rem_euclid(period)` で負値・複数周期を正規化する。
  各 contour は同じ offset から開始する。閉 contour の閉じ辺も弧長に含め、途中で phase を再開しない。
  終端と始点がともに描画中なら fragment を結合し、始点に cap を作らず join を作る。
  全周が一つの描画区間なら閉 fragment にする。端点位置は入力点をそのまま使用する。
- dash 内の既存頂点は保持し join を付ける。dash の両端には選択した cap を付ける。
  zero gap は描画区間を切断しない。zero dash は一点 fragment とし、butt は無被覆、
  round は半線幅の円、square はローカル X/Y 軸に平行な一辺線幅の正方形とする。
  zero dash の square は方向を持たない点としてこの規約を使う（一般の square cap は接線方向）。
  開 contour の終点上の zero dash も含める。幅 0 は cap を含め無被覆。
- 負長・非有限長・非空の全 zero 配列・周期 overflow は `InvalidDashArray`。
  JSON の非有限数は `FiniteF64` / JSON decoder が拒否する。未知・不正な保存 payload の opaque 保持を
  型付き編集が成功したことと混同しない。
- 入力配列は 256 要素以下。全 contour 合計で subdivision loop 内の進行 step / zero entry 訪問を
  16,384 以下、出力 fragment 数も 16,384 以下に制限する。
  境界をまたぐ path segment も step に数えるため、これは dash 個数だけより保守的な work 上限。
  上限超過、丸めで弧長が前進しない場合は `StrokeBudgetExceeded` / `STROKE_BUDGET_EXCEEDED`。
  無限 loop、clamp、実線への置換はしない。
- offset は `kronello.shape.dash_offset`（Scalar、DesignPx、既定 0、範囲制限なし）の通常 Property。
  Constant / Curve / Expression / Modifier、正規化有理数時刻、instance の local time を共通 evaluator で扱う。
  配列・alignment は Shape の型付き編集、offset の曲線・keyframe は既存の共通編集 command を使う。

## 線位置と affine

version 2 はローカル空間で中央線の矩形・join 三角形・cap 円を作り、それを affine で写す。
実装は同じ float32 primitive を CPU / WGSL に渡し、出力の各 AA sample を同じ逆 affine でローカルへ戻す。
非一様 scale で円は楕円、skew で矩形は平行四辺形になり、幅はその変換に追従する。
出力画素の一定幅ではない。reflection も同じ規約。特異・float32 lowering 後の非有限 / 近退化行列は
型付きエラー。逆写像の各 row は全出力領域で積の絶対値和 + translation の絶対値を `1e12` 以下に制限し、
finite coefficient の乗算 overflow を sampling 前に拒否する。GPU/CPU 共通境界で、最大 linear 成分による正規化 determinant の絶対値を `1e-6` より大きく要求する。

center は幅 w の中央線。inside / outside は幅 2w の中央線を作り、
それぞれ全 closed contour の fill-rule interior / その補集合との交差を被覆とする。
fill paint の有無とは独立に options.fill_rule を使う。穴、逆 winding、self-intersection は
既存の half-open Y crossing / strict cross の fill 判定と同じ。境界もこの判定で二分し、epsilon を足さない。
開 contour が一つでもある inside / outside は `OpenStrokeAlignment` / `STROKE_OPEN_ALIGNMENT`。
破線による開 fragment は元の閉 contour の interior で clip する。

layout bounds は元の解析幾何 envelope のまま。ink は局所の保守的 cap / join support を含めて
world affine で写し、visual はその後 effect support を合成する（ADR-0057）。
inside の外向き halo は 0、center は既存 cap / join halo、outside はその 2 倍。
ダッシュの空白、透明 paint、穴による tight bounds の縮小はしない。
DAG の pixel bounds は局所 halo に affine の各 row の絶対値和を掛け、出力格子で外向きに包含する。
ROI / tile は既存の出力 sample lattice を維持する。

## snapshot・cache・golden

ローカル輪郭の geometry cache は dash / paint 変更で再利用する。
raster identity は実際の stroke 幾何版、配列、評価済み offset、alignment、fill rule、
局所 fragment、forward / inverse affine、幅 / join / cap / limit を含む。
旧 stroke の raster key には旧文字列を保持する。
golden draw manifest には実際の版と全局所 fragment / dash / alignment / inverse を記録する。
4 新規シーンを追加し、旧 36 シーンの入力 geometry / paint は維持する。

ADR-0066 の不連続点規約を維持する。線辺・cap 円周・fill boundary 上の sample は
CPU / WGSL の FMA 有無で判定が分かれ得るため、新 fixture は fractional geometry / phase を使い、
exact tie を狙わない。許容誤差 `2^-10` を広げず、エッジ除外もしない。
CPU / Naga / schema の合格は Metal / baseline 比較の証拠ではない。
[VEC-005 検証](../testing/vec-005.md) の host コマンドで確認後、supervisor が候補を明示採用する。
