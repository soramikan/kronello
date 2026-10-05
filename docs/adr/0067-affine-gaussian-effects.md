# ADR-0067: 非一様 affine の Gaussian / shadow を明示した意味版 2 で扱う

- 状態: 採用
- 日付: 2026-10-05
- 対象: FX-002
- 部分置換: [ADR-0057](0057-layout-bounds-stages.md) の非一様 Gaussian 拒否と visual support の条項に、effect version 2 の規則を追加する。version 1 と他の bounds 規則は維持する。

## 背景

FX-001 の version 1 は world transform の等方倍率で sigma を写し、出力 X / Y の separable convolution を実行する。正の sigma の非一様 scale / shear は `UNSUPPORTED_FEATURE`。既存の作品・固定 snapshot を同じ意味版のまま新しい kernel へ移すと画素が変わる。

## 決定

- 保存 `EffectDefinition.version = 1` は既存の `fx001-separable-gaussian-transparent-rne16-v1`、既存変換制限、二つの中間面の RNE を維持する。自動移行しない。version **2** は同じ effect id と PropertyId parameters を使う明示選択とする。
- version 2 の局所 Gaussian は sigma を持つ等方分布。局所円形 3-sigma support を world linear transform `A` と出力倍率 `S = diag(sx, sy)` で写す。出力 covariance は `C = sigma² (S A)(S A)ᵀ`。translation は covariance に入れない。shear の cross term を保持し、軸別 separable kernel へ近似しない。
- `ResolvedEffect::AffineGaussianBlur / AffineDropShadow` は sigma と linear matrix を保持する。Property 解決直後の matrix は identity、`map_effect` で world matrix を合成する。shadow offset は `S A offset`、sigma と同じ局所空間。straight color / opacity、source alpha の使用、source の下への合成順は version 1 と同じ。
- kernel 版は **`fx002-affine-ellipse-lattice-rne16-v2`**。`C = [xx, xy, yy]` を f64 に固定する。整数出力 offset `d` の Mahalanobis distance `q = (Cyy dx² - 2 Cxy dx dy + Cxx dy²) / (Cxx Cyy - Cxy²)` を f64 で直接計算し、`q <= 9` の tap に `exp(-q/2)` を与える。Y → X の昇順で列挙し、f64 の総和で正規化して f32 weights に固定する。CPU と GPU は同じ tap 列を消費し、実行時の f32 weight 総和で除算する。sigma 0 は中心 `[1]`。
- version 2 の shadow sampling は `shift = floor(-offset)`、`fraction = -offset - shift` を先に求め、整数 pixel coordinate に shift を足す。大きい座標から offset を f32 で直接減算して fraction を作らない。tile 原点による丸め差を防ぎ、CPU / GPU で同じ四つの bilinear taps を使う。version 1 の算術は維持する。
- これは出力格子での分布のサンプリングであり、pixel 面積の厳密積分ではない。小さい sigma は中心 tap のみになる場合がある。version 1 の局所四角 support / separable 二段処理と区別する。version 2 は入力面 RNE → 二次元 convolution 一段 / RNE → shadow bilinear・source-over / RNE。透明な外部境界、premultiplied 作業用線形色、binary16 ties-to-even は維持する。
- semantic visual bounds の軸別連続 halo は `3 sigma hypot(A[i][0], A[i][1])`。pixel halo は `ceil(3 sqrt(Cii))` の保守的包含矩形。shadow は transformed offset を足した support と source の union。backward ROI は負 offset を適用し、bilinear taps の floor / ceil を含めて halo を広げる。stack を逆順に伝播し、出力格子を移動しない。
- 有限で非退化の行列だけを扱う。`m = max(abs(Aij))` とし、`abs(det(A/m)) > 1e-6` を要求する（sigma 0 も同じ）。正の covariance は `mC = max(Cxx,Cyy)` に対し `det(C/mC) > 1e-12` を要求する。距離計算に使う直接 determinant `Cxx Cyy - Cxy²` は正の normal f64 を要求し、underflow / subnormal / overflow も拒否する。support 境界 `q = 9` の tap に epsilon を加えたり、別の正規化演算で境界を動かしたりしない。特異 / 近退化、非有限、正 sigma の全 covariance underflow は `UNSUPPORTED_FEATURE`。clamp、等方近似、sigma 0 への置換をしない。
- 各 pixel radius は 1,024 以下。kernel の探索矩形 `(2rx+1)(2ry+1)` は **65,536 candidates 以下**。採用 tap だけでなく探索領域を allocation 前に検査し、超過は `UNSUPPORTED_FEATURE`。既存 16-entry stack / surface memory / device limits を維持する。shadow offset / opacity 不正は既存 render の `RENDER_ERROR`（GPU 境界では `INVALID_INPUT`）。性能目標 OQ-14 を決めない。
- `SemanticVersions.effects` は対応版上限を id ごとに固定する。新規 snapshot / capabilities は 2、旧 snapshot の 1 も実行できる。1 を固定した snapshot は authored version 2 を拒否する。実際の algorithm は常に各 `EffectDefinition.version` で選ぶ。未知版を拒否する。cache identity は実際の kernel version、effect semantic version、covariance / offset を含む。

## 検証と影響

[FX-002 検証](../testing/fx-002.md) に CPU 解析・impulse・crop / tile と GPU 比較を対応付ける。GPU 実測と Apple Silicon + Metal golden の生成・採用・比較は supervisor の host run が必要。CPU 合格を GPU の受け入れ完了と扱わない。

API schema の `ResolvedEffect` に二つの variant が追加される。保存 parameters / project schema / snapshot schema の構造版は変更しない。明示した version 2 への編集は既存 shared edit API を通す。version 1 golden の画素を維持し、4 新規シーンの候補は clean commit 後に生成する。
