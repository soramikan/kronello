# 05 レンダラーと GPU

## NLE-002 の動画 / Generator / clip effects

Sequence compiler は Composition の独立 instance に加え、動画 Asset の明示 stream / rational PTS 要求と `kronello.solid` version 1 を同じ Scene IR / Render DAG へ lowering する。Clip.properties は配置 transform / effects を Sequence time で評価する。動画は native dimensions、Generator は Sequence extent を local rectangle とする。track 順は下→上、transition の同一 track 内は開始時刻順。effects の後に crossfade incoming の opacity を掛ける。

`VideoRenderBackend` は固定 DAG に入った Asset hash / locator だけを解決する。元 Project を再読込しない。software seek / decode、明示 SDR RGBA8 color conversion / inverse transfer / premultiply、CPU nearest sampling、選択 GPU への明示 RGBA16F upload を通す。未知 format / HDR / 色 tag、asset 欠落 / hash mismatch は typed error。タグ欠落時の明示 default（YUV: BT.709 limited、RGB: sRGB full）は sequence.query に assumptions として見える。native plane decode API の HDR 保持をこの SDR renderer の対応と同一視しない。

clip effects は FX-001 / FX-002 の版付き ordered DAG、affine 契約、halo / backward ROI、raster identity を共有する。`FrameMetadata.input_path` は active video の CPU decode / color / sampling と selected backend の経路を表し、GPU の低層 TransferStats は upload bytes / operations を数える。GPU-resident decode、frame interpolation、tile 間の decode cache は追加していない。CPU / GPU 比較の実行範囲は [NLE-002 の検証](../testing/nle-002.md)、時間・色・Generator と transition の意味版は [ADR-0062](../adr/0062-video-generator-and-timeline-edits.md) を参照する。

## レンダー要求

```text
RenderRequest:
  snapshot_hash
  output_port
  instance_path
  sample_time
  spatial_region
  render_scale
  sampling_scope / shutter_policy
  quality_profile
  color_pipeline_id
  required_features
```

ノードは要求に応じて必要な入力時刻・領域を返す。出力ポートは Color / Mask を初期実装し、Depth / MotionVector / Normal は将来の型として境界を確保する。
値の評価と GPU コマンド発行を分離する。純粋モデル層は `wgpu::Texture` や `AVFrame` を保持しない。

NLE-001 の `RenderTarget::Composition / Sequence` は `render.frame` / `render.sequence` の両方で使う。Sequence を ClipId による独立 instance と active_range を持つ実行用 Composition に lower し、既存 Scene IR / DAG で track の下→上に合成する。空白区間は透明。保存文書の Composition を追加・変更せず、snapshot の owned Project に元の Sequence と全配置を固定する。Sequence の working_space が profile の正本で、復元 snapshot の不一致は拒否する。CPU-reference は明示指定し、GPU の暗黙 fallback はない。画像連番には音声を含めず、音声 Bus は service の `mix_sequence_audio` へ明示入力する。NLE-001 時点では Asset / Generator 動画 Clip と clip effects を延期し、NLE-002 が追加した（冒頭の節を参照）。Sequence A/V mux は後続範囲。[ADR-0051](../adr/0051-nle-placement-and-retime.md)、[検証記録](../testing/nle-001.md) を参照。

render は Scene IR と評価値を受け取り、具象 backend の実装を上位から渡された契約越しに呼ぶ。コード依存の向きは [ADR-0043](../adr/0043-semantic-dependencies-and-units.md) に従う。出力領域は左上原点の画素単位で、画素 `(i, j)` の中心は `(i+0.5, j+0.5)`。設計単位 `design_px` と区別する。

必要機能は要求と依存グラフから導出し、`required_features` と合わせて固定 snapshot の構造・意味の版を検証する。必要な未知機能・意味の版があれば最終レンダーを `UNSUPPORTED_FEATURE` で拒否する。プレビューの警告付き代替結果は最終結果・キャッシュと区別する（[ADR-0045](../adr/0045-snapshot-compatibility-boundaries.md)）。

OpenFX の入力領域 / 必要フレームの問い合わせに似た契約を参考にするが、OpenFX ホスト互換をこの段階で約束しない。

## 合成

- 内部の色付き画像は、明示した作業用線形空間の premultiplied alpha とする。保存する Color Property の straight RGB と区別する。作業用色空間は下の「色」を参照。
- Group の既定は、子を一度まとめてから Group opacity / 効果を適用する isolated 方式。
- 各子に opacity を配る最適化は意味が一致する場合だけ行う。
- Blend mode が表示基準の色空間を必要とする場合は明示変換を置き、すべて線形で同じ見た目になるとしない。
- opacity / coverage は premultiplied RGB と alpha の両方に掛け、画像の補間・blur・蓄積も premultiplied 値で行う。alpha / coverage は有限の `[0, 1]`、alpha = 0 の内部 RGB はゼロとする。HDR RGB の負値・1 超を alpha の範囲に clamp しない。
- 外部アダプターは `straight / premultiplied / opaque` と関連付け空間を明示する。非線形色変換は straight RGB に行い、外部入出力の unpremultiply 時は `a > 2^-16` で除算、それ以下は RGB をゼロとし alpha は保持する。内部 effect の unpremultiply はゼロだけを特別扱いし、内部画像に閾値を適用しない。
- alpha を持たない出力は明示した背景へ合成する。外部 alpha 変換、閾値の境界、マット境界を検証する。詳細と新規に固定した契約は [ADR-0044](../adr/0044-color-and-alpha-contracts.md) を参照。

### VEC-005 の版付き stroke coverage

`vec003-centered-stroke-v1` は従来の output-space 展開を維持する。
明示 `Stroke.options` の `vec005-local-stroke-v2` は bounded dash subdivision 後、
局所矩形・三角形・円を CPU / WGSL で共有し、AA sample の逆 affine で被覆を判定する。
inside / outside の fill-rule clip は paint の有無に依存しない。
非一様 scale / skew / reflection の線幅は局所線に変換を適用した幅になる。
semantic bounds は局所 support を変換し、pixel bounds / backward ROI は row の絶対値和で halo を包含する。
snapshot は旧 stroke 版も認識するが、旧版に固定した snapshot で新 options を実行しない。
cache / golden draw manifest は実際の版と dash / phase / alignment / local primitive 入力を固定する。
[ADR-0073](../adr/0073-local-stroke-extensions.md)、[VEC-005 検証](../testing/vec-005.md) を参照。
Metal parity / baseline 採用は host run 待ちであり、CPU・Naga 合格とは区別する。

### VEC-004 の gradient paint

[ADR-0066](../adr/0066-explicit-gradient-semantics.md) の版 1 options を各 gradient に保持する。
coverage サンプルの座標を node / ROI の逆写像、各 gradient の bbox / affine 逆写像で gradient 空間へ移す。
parameter → pad / repeat / reflect → stop の補間 → 作業用線形 premultiplied paint の順。
補間空間と alpha association は独立の意味で、sRGB straight / premultiplied 補間も合成前に decode する。
text も同じ shader / CPU reference の paint 経路を使い、shaping cluster を作り直さない。
sampling の座標・焦点円の判別式・NaN parameter・周期 spread の無限 parameter は、
CPU の面検証と GPU の sticky validation flag で型付きエラーにし、stop 色へ置換しない。
旧 pad の無限 parameter は VEC-003 と同じ端点色を維持する。

`SemanticVersions.gradient_interpolation` は `vec004-explicit-interpolation-v1`。
個々の `interpolation_version` と全 options / stop / transform を raster identity に含める。
旧固定 snapshot は意味版の不一致を拒否し、旧 Project の省略 options は従来値へ正規化する。
16bit PNG / RGBA16F は従来の出力規約を維持し、VEC-004 では dither を追加しない。
native preview の Bgra8Unorm は banding の可能性を残す。8bit 出力 / preview の対策は将来の量子化境界で検討する。
Metal 実機の一致・32 シーンの新 golden 候補生成 / 明示採用 / 比較は host run 待ち。
[検証記録](../testing/vec-004.md) の残件を完了するまで GPU の受け入れ成功と扱わない。

## 色

[ADR-0024](../adr/0024-working-color-space.md) の決定を維持し、値の表現・alpha・変換境界を [ADR-0044](../adr/0044-color-and-alpha-contracts.md) で固定する。

### 作業用色空間

| 作業用色空間 | 用途 | 既定になる条件 |
|---|---|---|
| 線形 Rec.709（sRGB 原色、D65） | SDR | 新規 Sequence の既定 |
| 線形 Rec.2020（D65） | HDR・広色域 | HDR / 広色域の出力プロファイルを選んだ Sequence |

- 作業用色空間は Sequence ごとに持つ（`Sequence.working_space`）。初期実装の選択肢は上の 2 つ。ACEScg などは後から追加できる拡張とする。
- 中間画像は RGBA16F。範囲外の値（1.0 超、負値）は出力変換まで保持し、途中で切り捨てない。
- Composition は固有の作業用色空間を持たず、配置先の Sequence の作業用色空間で評価する。Composition 単体のプレビューやレンダーでは、要求側が色パイプラインを指定する（省略時は線形 Rec.709）。
- 作業用色空間は `color_pipeline_id` に含まれ、キャッシュキーの一部になる。Sequence の作業用色空間の変更は意味の変更として扱い、画素キャッシュを無効化する。

2 つの作業用色空間は互いに線形変換の関係にあるため、線形補間と通常の加算的な合成は、範囲の切り捨てが起きない限りどちらで計算しても同じ色になる。差が出るのは、非線形な blend mode、エフェクト内の切り捨て、8bit ラスタライザー経路などである。

### 色の値

- 保存する色は `{space, components}` の明示表現とする。色空間を持たない色を保存しない。
- 色空間の指定がない色入力（`#F59E0B`、0〜255 の RGB 値など）は sRGB（非線形符号化）として解釈し、保存時に明示表現へ正規化する。
- 評価時に、保存された色を Sequence の作業用色空間へ変換する。同じ色入力は、作業用色空間が違っても同じ色を意味する。
- Color の補間は既定で作業用線形空間で行う（[03 プロパティとアニメーション](03-property-animation.md)）。
- 保存 RGB は straight で alpha は独立、省略入力 alpha は 1。8bit RGB は 255 で正規化する。非線形 sRGB と線形 Rec.709 を同じ数値表現と扱わず、sRGB の伝達関数を復号する（gamma 2.2 やタグの付け替えで代用しない）。alpha は伝達関数の対象にしない。

### HDR

方針は [ADR-0037](../adr/0037-hdr-policy.md) による。実装と検証は M4（COLOR-001）。M0〜M3 は SDR のみを扱う。

- 基準白は 203 cd/m²（ITU-R BT.2408）。
- HDR 出力は Rec.2100 の PQ と HLG。
- SDR の素材と色入力の白は、基準白に合わせて配置する。
- HDR の作業値 1 は基準白に対応し、1 超を保持する（ADR-0044）。この規約の採用だけで M0〜M3 の HDR 対応を宣言しない。
- トーンマッピングと色域圧縮は、プレビューの表示変換と、明示的な SDR 変換出力でだけ行う。HDR 出力へは焼き込まない。

## 基本エフェクト

FX-001 は `SceneNode.effects` の順序付き stack と `DagNode::Effect` を実装する。sigma / offset / color / opacity はノード所有 Property で、評価済み `ResolvedEffect` を Scene IR に保持する。blur はローカル `design_px` の sigma を変換・出力倍率で画素へ写し、`radius = ceil(3σ)` の正規化 Gaussian を水平・垂直に畳み込む。透明 edge mode、内部線形 premultiplied RGBA16F と明示した binary16 RNE 面境界（CPU oracle も同じ丸め）を使い、shadow は blurred source alpha にタグ付き straight 色・opacity を掛けて source の下へ合成する。version 1 は等方変換に対応し、正の sigma に対する非一様変換は型付き未対応。この旧版の画素・制限は維持する。

FX-002 は同じ effect id / parameters の明示した **version 2** を追加する（[ADR-0067](../adr/0067-affine-gaussian-effects.md)）。局所 Gaussian の covariance を `C = sigma² (S A)(S A)ᵀ` として出力格子へ写す。rotation / reflection / 非一様 scale / shear の cross term を保持する。整数 offset の `q = dᵀ C⁻¹ d <= 9` に `exp(-q/2)` の重みを与え、正規化した同じ f32 tap 列を CPU / GPU が一段で畳み込む。kernel 版は `fx002-affine-ellipse-lattice-rne16-v2`。sigma 0 は中心 tap、shadow offset は `S A offset`。shadow sampling は `floor(-offset)` の整数移動と offset だけから求めた fractional taps を分け、tile 原点で fraction が変わらない。面境界の binary16 RNE と transparent edge は共通。旧 version 1 から自動移行しない。

semantic visual halo は軸別 `3 sigma hypot(A[i][0], A[i][1])`、pixel halo は `ceil(3 sqrt(Cii))`。shadow の bilinear floor / ceil と source union を含めて逆 ROI を要求する。有限非退化行列だけを扱い、normalized determinant `> 1e-6`、normalized covariance determinant `> 1e-12`、距離計算に使う直接 covariance determinant は正の normal f64、各 radius `<= 1024`、探索矩形 `<= 65,536 candidates` を要求する。超過・特異・近退化・covariance underflow は `UNSUPPORTED_FEATURE`。近似や clamp はしない。既存 surface memory 予算も適用する。CPU / GPU 比較と golden の実測状態は [FX-002](../testing/fx-002.md) に記録する。

`PixelEffect::required_input` が output → input ROI を宣言し、DAG の逆順で Group / mask / 共有入力へ union を伝播する。初期 executor は必要領域の union を元の画素格子で描画し、要求画素へ crop する。`RenderDag::bounds()` の ink / visual は変換と離散 halo を含む output pixel bounds。effect params・意味版・upstream identities・ROI・色・backend namespace を cache key に含める。metadata は effect id ごとの対応版上限（新規 2 / 旧 1）を固定する。各 authored effect の版が algorithm を選び、上限 1 の snapshot に版 2 は入れない。GPU / golden の採用検証は [FX-001 の検証記録](../testing/fx-001.md) に分けて記録する。

## 高解像度

RGBA16F の 3840x2160 は 63.28125 MiB、7680x4320 は 253.125 MiB（画像データのみ）。

- デコード面、参照フレーム、中間テクスチャ、字形アトラス、蓄積バッファ、エンコーダーを別に予算化する。
- macOS の共有メモリを独立した VRAM と同じ予算計算にしない。
- CPU / GPU 往復、GPU 内コピー、FrameBridge の同期待ちを計測する（[ADR-0008](../adr/0008-explicit-cpu-gpu-transfer-paths.md)）。
- Vulkan / Metal / D3D12 との相互運用は専用モジュール（`kronello-framebridge`）に隔離し、参照デバイス・所有権・同期の契約をテストする。

### GPU 経路の保証

M0 で GPU interop の困難さを確認するが、zero-copy の完全達成を M1 の CPU 検証版まで阻害する必須条件にはしない。
M1 / M2 は互換経路でも実装を進め、転送コストを明示する。GPU 経路の保証はプラットフォーム / 形式ごとに昇格する。最初の保証経路は macOS の Metal + VideoToolbox（[ADR-0015](../adr/0015-macos-first-platform-priority.md)）。
非対応経路は明示的な fallback とするか、`require_gpu_resident` 指定時はエラーにする。

## モーションブラー

- 一般の幾何アニメーションは複数サブフレームで評価できるようにする。
- 露光時間は `shutter_angle / 360 / output_fps` と定義し、シャッター位相も設定に含める。
- 基準実装は、各サブ時刻で Composition 全体を合成してから重み付き平均する。各レイヤーを個別に平均してから重ねるだけでは重なりの意味が変わりうるため、無条件な置換をしない。
- NLE のカット境界を跨ぐブラーは既定で避け、クリップ境界の方針を明示する。
- ネストごとにサンプル数を掛け合わせないよう、sampling_scope と共通時刻要求を共有する。
- サンプルを順次蓄積して、全サンプル画像を同時保持しない。
- 動画内の被写体の真のサブフレーム像が復元されるわけではない。オプティカルフローは別モジュール。

## キャッシュ

### INSPECT-001 の実行前説明

共通 Query `render.explain` は Composition / Sequence の snapshot、Scene IR、tile ごとの Render DAG を組立て、stage code / inputs / SceneKey、要求・halo 実行領域、面数・メモリ・転送の推定を返す。frame executor と同じ `frame_tiles` を使う。`executed:false` であり、GPU の可用性や転送時間の実測ではない。失敗時は `plan:null` と型付き diagnostics を返し、代替 backend を選ばない。

control upload / image upload / GPU image copy / image・status readback を分ける。組込 GPU の frame export は tile ごとに linear / display の二回描画を行い、二つの RGBA16F image（256-byte row padding）と二つの4-byte statusを readback する。control upload の bytes / operations は未推定の null。CPU の GPU 転送は0。注入 backend の未知使用量も null とする。

RGBA16F 面は8 bytes/pixel、CPU 参照面は16 bytes/pixel、最終 host の linear / display は合計32 bytes/pixel。中間面は既存 backend の保守的な安全予算式による `_estimate` で、allocator / driver / geometry / font / RSS / 実測 peak を含まない。`DUPLICATE_LINEAR_DISPLAY_RENDER`、`ZERO_OPACITY_STILL_PROCESSED`、`EFFECT_HALO_EXPANSION`、`SURFACE_BUDGET_EXCEEDED` は処理・安全予算上の notice。OQ-14 の性能合否を決めない。

Query ごとの隔離 `RenderCache` の実 compilation counters を `compilation_cache` に載せ、renderer の cache / LRU / counters を変更しない。scope は `isolated_query_compilation`、raster 未実行を `raster_cache_observed:false` で明示する。runtime の warm hit/miss と混同しない。非表示原因は `node.explain` が containment・transform parent・opacity・active range・transient matte・content / font / unsupported の別に返す。画素の occlusion・coverage は測定しない。詳細は [ADR-0060](../adr/0060-structured-read-only-inspection.md)、[INSPECT-001 検証](../testing/inspect-001.md)。

```text
cache_key = hash(
  node semantic version,
  dependency content hashes,
  instance input hashes,
  local sample time + time-map version,
  requested region + scale,
  quality + AA + shutter configuration,
  font/layout/vector versions,
  color pipeline,
  seed,
  simulation checkpoint identity when needed
)
```

- Property 値、Layout、Geometry、Raster、Effect frame、Simulation checkpoint を別キャッシュにする。
- 構造変更で compile、値変更で該当部分評価、色変更で組版再利用という粒度を目標にする。
- 厳密モードの画素キャッシュはデバイス・ドライバー・エンジンの fingerprint で分ける。
- 時間依存や Simulation の変更では将来方向への無効化を適切に広げる。

レンダーキャッシュは作品データではない。`.kronello` の外（OS のキャッシュ領域）に置き、削除しても作品を失わない（[ADR-0006](../adr/0006-document-vs-render-cache.md)）。

## M0 GPU スパイクの実装範囲

GPU-001 の `kronello-gpu` は wgpu 30.0.1 / pollster 1.0.1 を使い、矩形・PAM 素材から線形 premultiplied RGBA16F までの最短経路を実装した。CPU upload / GPU 内コピー / GPU→CPU readback を別の `TransferStats` として記録する。`kronello-framebridge` の unsafe native interop は macOS のモジュール内に隔離し、IOSurface の BGRA8 単一面取り込み・出力を検証する。M0 スパイク自体は通常 renderer / Render DAG を提供しない。M1 の RENDER-001 は下記の節で接続し、他形式の GPU 常駐保証は未実装。実測結果と制約は [スパイク報告](../testing/gpu-spike-m0.md)、基準未登録の golden harness は [比較手順](../testing/golden-comparison.md) を参照。

M1 / Metal の追加実測では、wgpu 30.0.1 の同一 MTLDevice による IOSurface の零コピー import / output と、CVPixelBuffer → CVMetalTextureCache → HAL import が成功した。VideoToolbox の H.264 decode 出力は BGRA8 および NV12 biplanar（R8 / RG8）を取り込めた。H.264 3 frame は両形式で hardware decoder 使用を確認し、BGRA8 のテストパターン最大 channel 誤差は 1。wgpu 出力の VideoToolbox encoder 投入と YCbCr→RGB 精度は未検証。形式ごとの実測値・寿命・同期・転送 counters の範囲は [追加スパイク報告](../testing/gpu-spike-m0.md#corevideo--videotoolbox-追加スパイク) を参照。

## M1 GPU-002 の描画境界

`kronello-gpu::DrawScene` は plain data の `DrawNode` と root のノード参照を受ける。`PathDraw` は出力要求に合わせて flatten・変換済みの `Contour`（design_px）とタグ付き straight `Paint`、`Fill`、`RoundStroke` を保持する。store・フォント探索には依存しない。StrokeJoin / StrokeCap は backend-free なモデル enum を共有する。呼出側が `kronello-vector::flatten` の polyline、または `kronello-text::layout` の positioned outline を flatten した結果を写す。未変換の曲線・モデルの stroke style を暗黙に解釈しない。

### Coverage の定義（vec003-grid4-v2）

- GPU compute で各画素の 4×4 固定サンプルを検査する。画素内位置は `((sx+0.5)/4, (sy+0.5)/4)`、`sx, sy = 0..3`。出力画素から design_px への倍率は `RenderSize` で指定し、被覆率は hit 数 / 16。ハードウェア MSAA のサンプル配置には依存しない。
- fill は Nonzero / Evenodd。開いた contour も fill では暗黙に閉じる。水平辺は winding に寄与せず、Y crossing は半開区間、cross product の符号は厳密な正負で判定する。点が辺上にある場合も同じ判定規約を使う。
- stroke は miter / bevel / round、butt / square / round に対応する。共通の CPU 展開で中央線の矩形・接合三角形・円を生成し、GPU へ同じ領域を渡す。miter limit（既定 4）を越えた接合は bevel。閉じる辺は `Contour.closed` のときだけ stroke に含める。幅 0 は無被覆。幾何定義・退化処理は [04 章の VEC-003 規約](04-vector-text-layout.md#vec-003-の実装規約) を参照。dash・stroke alignment は未対応。
- fill と stroke は別々に coverage を resolve し、stroke を fill の上に source-over する。単色または線形 / 放射 gradient の各 stop は sRGB decode → Rec.709 / Rec.2020 原色変換 → premultiply の順で処理する。各サンプル位置の線形 premultiplied paint を被覆に応じて蓄積・平均する。coverage に伝達関数を適用しない。RGBA8 の色付き raster を経由しない。
- `render_scene_reference` は同じサンプル配置・辺判定・stroke・描画順の CPU 参照。coverage の CPU は binary16 丸めを行わない（FX-001 の effect 面境界は別に明示した RNE を行う）。AA は画素面積の厳密積分ではなく、この版付きサンプリング契約。辺を比較から除外しない。GPU の各中間面は RGBA16F。

### Group、mask、外部出力

`DrawNode::Group` は子の source-over を透明な offscreen RGBA16F へまとめ、opacity を RGB / alpha に一度だけ掛ける。ネストも同じ手順。`DrawNode::Masked` は source と matte の参照を入力とし、matte を表示順に自動挿入しない。共有入力は要求内で一度描画して再利用する。matte 自身を表示したい場合だけ root / children に明示する。

`MaskKind::Alpha` は matte alpha、`MaskKind::Luminance` は作業用線形空間の premultiplied RGB から求めた Y（straight Y × alpha）を `[0,1]` に clamp して coverage にする。Rec.709 は `(0.2126, 0.7152, 0.0722)`、Rec.2020 は `(0.2627, 0.6780, 0.0593)`。alpha を二重に掛けず、encoded sRGB の明度を使わない。色入力を coverage に変える明示ノードであり、既存 coverage を再度色変換するものではない。

`GpuContext::render_scene` は作業用線形 premultiplied `RenderOutput` を返す。`render_scene_output` は `OutputTransform`（sRGB / linear Rec.709 / linear Rec.2020、straight / premultiplied）を必須とし、外部 epsilon に従う unpremultiply → 原色変換 → 必要なら sRGB encode → 指定空間で再 premultiply の順で処理する。`ExternalFrame` に関連付け空間を明示し、encoded premultiplied を内部画像と混同しない。alpha の破棄・背景の推測・HDR の PQ/HLG・tone mapping は提供しない。RGB は clamp せず、各 RGBA16F 面への書き込み前に有限・alpha 範囲・RGB の絶対値 65,504 以下を検証する。GPU は全 pass で共有する sticky status に失敗を記録し、Metal 等が範囲超過値を有限最大値へ飽和させても型付きエラーを返す。後の不透明描画や group opacity で隠れる中間 overflow も拒否する。CPU 参照も各面境界で同じ範囲を検証する。binary16 alpha がゼロへ丸められるときだけ RGB もゼロに正規化し、正の内部 alpha に外部 epsilon を適用しない。

scene は参照欠落・循環・不正 opacity / 色 / 幾何を型付きエラーで拒否する。保守的な予算は 1,024 nodes、各 group / roots 1,024 references、合計 65,536 edges、深さ 32、座標の絶対値・stroke 幅 1,000,000 以下、GPU 中間面の上限推計 512 MiB（CPU 参照にも float32 の面サイズで同じ予算を適用）。デバイス限界超過もエラー。GPU 不在では skip / CPU fallback しない。CPU upload は幾何と制御データのみで、合成・mask は GPU 常駐。`TransferStats` はこれらを control upload として計上し、最終 GPU copy / image readback と、4 bytes の validation status readback（1 回）を計上する。性能・資源プール・tiling・高品質 AA は今後の検証対象。

## M1 RENDER-001 の Scene IR / Render DAG と連番出力

`kronello-render` は store / GPU に通常依存しない。`RenderBackend` を呼出側から渡し、`kronello-gpu::GpuContext` の trait 実装で GPU-002 の coverage・隔離合成・mask・出力変換を実行する。`kronello-gpu::render_adapter::CpuReferenceBackend` は明示選択する float32 の参照実装で、通常の意味テストに使う。GPU adapter 不在を CPU で補う動作はない。render → gpu の参照はテスト用 dev-dependency だけとする（ADR-0043）。既存 GPU API は維持する。

### 固定 snapshot と公開 API

- `RenderSnapshot::new(&Project, CompositionId, revision, RenderProfile)` は文書を複製し、選択した Composition、revision、profile、必要な `FontRef` と意味の版を固定する。`RenderProfile` は作業用線形 Rec.709 / Rec.2020 と flatten tolerance（既定 0.02 output px）。元の Project を編集しても snapshot は変わらない。
- `RenderSnapshot::with_contract` は `SemanticVersions` と `MatteBinding` も明示入力する。公開 snapshot schema は **1**。Serde の strict な envelope を使い、復元時に欠けた版・lock を最新値で補わない。文書意味版・補間版・TimeMap 版・組版版は **1**、vector は `render001-kurbo-flatten-v1`、色は `gpu002-color-v1`、coverage は `vec003-grid4-v2`。現在の stroke 対応上限は `vec005-local-stroke-v2`、旧 `vec003-centered-stroke-v1` の pin も旧 stroke に限り認識する。gradient interpolation は `vec004-explicit-interpolation-v1`。実行時は対応する版と文書の意味版との一致を検証する。
- `content_hash()` は snapshot 全体を `serde_json::Value` の sorted object keys → compact UTF-8 → SHA-256 にする。STORE-001 の正規化規約を再利用し、独立した opaque 内容も hash に含める。schema、文書、revision、lock、profile、matte、意味の版を除外しない。time / region は個別の要求と metadata に保持する。CACHE-001 の values key は下記の rendering content identity と Time を使い、layout / geometry / raster はそれぞれ必要な内容だけで区別する。
- `build_scene_ir(&snapshot, Time, &[FontData])` と `build_render_dag(&SceneIr, RenderProfile, OutputRegion)` は GPU・ファイル I/O を使わない。
- `render_frame(&snapshot, &[FontData], &dyn RenderBackend, FrameRequest)` は `RenderedFrame`（作業用線形 premultiplied と外部 straight sRGB の画素、`FrameMetadata`）を返す。
- `render_sequence(&snapshot, &[FontData], &dyn RenderBackend, SequenceRequest, output_directory)` は同期 offline 出力を行い、`SequenceMetadata` を返す。CLI-001 / service が文書・font bytes・backend・出力先を渡す。素材の自動取得、システムフォント検索、store 参照は含めない。

実行対象は選択 Composition と CompositionInstance の定義依存閉包。依存しない opaque Composition / Curve / Shape / Text は実行しないが、内容は identity から落とさない。必要な opaque 内容と必要性を判断できない Project の未知フィールド、未対応の意味版は `UNSUPPORTED_FEATURE`。Expression は評価層の同じコードを伝播する。フォント欠落は `ASSET_MISSING`、hash / identity 不一致は `ASSET_HASH_MISMATCH`、欠落 glyph は `GLYPH_MISSING`。型・参照・範囲違反を未対応にまとめず、基底値・代替字形へ置換しない。

### IR、DAG、出力領域

`SceneIr` は有理数時刻、設計寸法、安定した `SceneKey`（InstancePath / NodeId）、containment 親、world transform、局所 opacity、評価済み Shape と Property 値、設計座標の `LayoutResult`、matte 入力を持つ。text の Path は既に glyph position を含むため、再度 glyph origin を加えない。評価層の active_range・ネスト時刻・入力束縛・描画順・transform-parent の結果を使う。

DAG は topological なノード列と明示 input index を持つ。index は要求内の導出参照で、文書 ID ではない。

| `DagNode` | 入力と処理 |
|---|---|
| `Geometry` | 評価済み Shape の意味的値と SceneKey |
| `TextLayout` | 設計座標の組版結果と SceneKey |
| `CoverageDraw` | Geometry / TextLayout を参照し、要求 scale で flatten した contour とタグ付き paint を描画 |
| `IsolatedComposite` | 順序付きの子を source-over し、局所 opacity を RGB / alpha へ一度だけ適用 |
| `Mask` | source / matte 参照。alpha または線形 working-space luminance の coverage |
| `OutputTransform` | 全 root の隔離合成を入力とし、線形 premultiplied と外部 straight sRGB を生成 |

`OutputRegion { origin, extent, pixels }` は設計座標の矩形を出力画素へ写す。`p = diag(pixels / extent) × (design_position - origin)`、左上原点・+Y 下向き。ROI / 解像度を変えても文書・組版は変えない。異なるアスペクト比を要求したときは、この明示写像で伸縮し、responsive variant の再組版を暗黙に行わない。flatten の最大拡大率は node world transform と ROI 写像を合成した行列の Frobenius norm で保守的に求める。

Shape の単色 / linear・radial・focal_radial・conic gradient fill / stroke と miter / bevel / round join、butt / square / round cap を接続する。stroke の非一様 scale / shear は `UNSUPPORTED_FEATURE`。fill / text の非一様変換は対応する。未知 paint / 後続 stroke 機能は opaque で保持し、必要な最終出力は拒否する。glyph ごとに coverage を作り、text の opacity は glyph 全体の合成に一度掛ける。Group / Null / 配置の containment 枠も局所 opacity と順序を保持する。

現行文書型には matte 欄がないため `MatteBinding` を snapshot の明示レンダー入力とする。source / matte とも stable SceneKey。matte は表示 root / children から除外し、`visible = true` の場合だけ表示する。source ごとの binding は一つ、共有 matte の DAG は再利用する。欠落・非アクティブ参照・containment / matte を合わせた循環は失敗する。

scene 1,024 node、DAG 4,096 node、containment / matte recursion 24、出力 16,777,216 pixel の保守的上限を設ける。backend は GPU-002 の 1,024 draw node・32 depth・65,536 edge・512 MiB 面予算をさらに適用し、限界を超えた要求はエラーにする。INTEGRATION-001 / [ADR-0053](../adr/0053-integration-evaluated-queries-and-render-tiles.md) で幅または高さが512 pixelsを超える出力を最大512×512のtileへ分け、元画素格子と既存effect ROI haloを保って同じbackendで実行する。metadataは元のregion、最終linear / display面は全画面のまま。movie export は RENDER-003 の tile sink から1枚の RGBA8 buffer に組み立てて即 encode し、全画面 linear / display と全 frames payload を保持しない。巨大 cumulative halo は node 別の保守的 allocation 総額512 MiBで backend allocation 前に拒否する。GPU texture cache・資源pool・性能保証は未実装。CACHE-001 のインメモリ cache は下記の範囲で実装した。

GPU adapter は同じ lowering 済み DrawScene について `render_scene` と `render_scene_output` を各一回呼ぶ。両経路とも合成・mask・色変換を GPU 上で行い、それぞれ image と validation status を readback する。CPU へ持ち帰った線形画素を出力変換する GPU 名義の経路ではない。二回の描画を統合する最適化と renderer API での転送統計の集約は後続課題。

### 連番・ファイル・metadata schema 1

`frame_samples(TimeRange, FrameRate)` は時刻ゼロの絶対格子を使う。frame index は `ceil(start × fps)..ceil(end × fps)`、時刻は `fps.frame_to_time(index)`。`[start, end)`、負 index、30000/1001 fps を有理数演算で処理し、浮動小数点時刻や前フレームへの加算は使わない。連番名は要求内の ordinal 0 始まり、絶対 frame index と区別する。空範囲の出力は空の manifest、一回の上限は 1,000,000 frame。

| 出力 | 符号化・用途 |
|---|---|
| `frame-00000000.rgba16f` | 数値の正本。header なし、row-major、上から下、RGBA、IEEE binary16 little endian。作業用線形 Rec.709 / Rec.2020、premultiplied alpha。RGB の負値・1 超を保持 |
| `frame-00000000.png` | SDR 閲覧用。RGBA 16-bit UNORM、big-endian samples、straight sRGB、sRGB chunk 付き。外部 epsilon 規約を使い、原色変換・sRGB encode 後だけ `[0,1]` に clamp。tone mapping なし |
| `frame-00000000.json` | `FrameMetadata`。上記二形式の区別と固定入力・要求・実行経路 |
| `sequence.json` | `SequenceMetadata`。range / fps / 格子規約と各 frame の metadata、ファイル名・byte count・SHA-256 |

RGBA16F の各 component は有限、alpha は `[0,1]`、RGB の絶対値は 65,504 以下。CPU oracle は float32 で計算し、raw 書出し境界で binary16 に丸める。alpha が binary16 のゼロへ丸められる場合だけ RGB もゼロにする。正の内部 alpha を外部 epsilon で落とさない。GPU は中間面も binary16 のため、CPU / GPU のビット一致は保証しない。PNG の clipping 画像を数値比較の代わりにしない。Rec.2020 数値出力は PQ / HLG / HDR tone mapping の対応保証ではない。

`FrameMetadata` の必須項目は次のとおり。

- `schema_version` / `snapshot_schema_version` / `project_schema_version`、`snapshot_content_hash`、元の `revision`（10進文字列）、選択 `target`。互換フィールド `composition` は Sequence の場合、lower した実行用 root の ID。
- `semantic_versions`（document / interpolation / time_map / layout / vector / color / coverage / stroke_geometry / gradient_interpolation、effects / generators の version map、video_input）、`font_locks`（family / PostScript 名 / hash / face index）。
- 正規化有理数 `time`（num / den は10進文字列）、連番時の `frame_index`（10進文字列）と `sequence_number`。任意時刻の still では後二項目は null。
- `design_extent`、`region`（origin / extent / pixels）、2×3 の `design_to_pixel`、`working_space`、`flatten_tolerance_px`。
- `numeric` / `display` の各 `ImageFormat`（color_space、transfer_function、alpha、association_space、pixel_format、channel_order、row_order、byte_order、clipping）。
- `backend`（`cpu_reference_float32` または `wgpu_rgba16f`）、`input_path`（通常の `semantic_scene`、または動画 CPU decode / color / sample と selected backend の明示経路）。厳密 cache の GPU / driver fingerprint 固定は CACHE / QA の後続範囲。

出力先は新しい directory を排他的に作り、既存 directory は拒否する。全 frame を内部 staging へ生成・検証・sync してから確定名に rename し、最後に `sequence.json` を確定する。通常エラーでは今回作った directory を rollback する。既存成果物は上書きしない。プロセス強制終了時の orphan 回収・resume・directory 全体の crash durability は JOB / RECOVERY の未実装範囲。

受け入れ条件と CPU / host 検証の区別は [RENDER-001 の検証](../testing/render-001.md) を参照。

## M1 CACHE-001 の分離 cache

`kronello-render::RenderCache` は呼出側が所有する削除可能な導出データで、Project / `.kronello` に保存しない。ディスク cache は追加せず、将来追加する場合も ADR-0006 と `kronello-store::render_cache_location` に従う。GPU / SQLite の型や pointer identity を key に使わない。

key は `cache001-json-sha256-v1` と level namespace を付けた入力を、sorted object keys の compact UTF-8 JSON にして SHA-256 で生成する。文書の編集 revision だけで区別しない。下流への伝播は content hash で行い、別ノードの entry を一括削除しない。過去の key の entry は容量内で残り、同じ内容に戻した場合も再利用できる。

| level | key の意味的入力 | 保持する導出値 |
|---|---|---|
| values | snapshot の全内容（revision だけ除外）、schema / semantic_versions、font lock、profile、matte、Composition と、完全な `RuntimePropertyKey`、正規化有理数 Time | 最終 `Value` |
| layout | 本文、style span の byte range / FontRef（hash・face・名前）/ size、wrap_width、line_height、alignment、direction、ruby、layout semantic version | paint を中立色にした `LayoutResult` |
| geometry | vector semantic version、評価済み geometry の形状種別・寸法・半径・Path、design-space flatten tolerance。glyph は layout content hash も含む | ローカル設計座標の `FlattenedPath` |
| raster | geometry content hash、変換済み contours、fill 色・色空間・fill rule、stroke 色・幅・join / cap / miter limit、fill / stroke gradient の geometry・stop 色 / offset・逆写像、OutputRegion、working space、vector / coverage / color / stroke_geometry / gradient_interpolation semantic version、backend 実行 namespace | 作業用線形 premultiplied float32 画素 |

geometry の scale bucket は丸めない exact tolerance とする。現行 flatten は `tolerance_px / conservative_magnification` だけに依存するため、同じ tolerance を持つ要求が同じ key を共有する。量子化による輪郭変化を導入しない。Position / Opacity は layout とローカル geometry の入力ではない。Rotation / Scale も layout に影響せず、必要な flatten tolerance や出力写像だけを変える。

layout の取得後に、その時刻の各 style の fill を `style_index` で glyph に付け直す。色変更だけでは組版・輪郭を作り直さない。本文・font lock・size・wrap・行高・alignment の変更は当該 layout と、その hash を持つ glyph geometry / raster を区別する。同じ outline を持つ別 font bytes でも lock の変更を下流へ伝える。warm layout hit でも明示 font bytes の hash・face・名前・重複を照合し、欠落・破損を成功に変えない。

各 level は独立の LRU で、`CacheConfig` の `CacheCapacity { entries, bytes }` に従う。既定は各 256 entries / payload weight 64 MiB。bytes は allocator overhead を含む process RSS の保証ではなく、layout / geometry / raster の保持データ量と Value の JSON byte 数を用いた重みである。単一 entry が byte 上限を超えた場合は保持せず正常に計算する。容量 0 でも同じ意味の結果を計算する。失敗した計算は保持しない。

`stats()` は各 level の hits / misses / inserts / evictions と現在 entries / bytes を返す。`clear()` は entry を削除し、累積 counter は残す。`reset_stats()` は entry を残して counter をリセットする。

### 公開経路と backend

- `build_scene_ir_with_cache`、`build_render_dag_with_cache`、`render_frame_with_cache`、`render_sequence_with_cache` は `&mut RenderCache` を明示入力する。frame / sequence 間で同じ cache を渡せる。既存 API は zero-capacity cache を使い、戻り値と metadata の意味を維持する。
- `RenderSnapshot::evaluation_content_hash()` は `content_hash()` と同じ入力から revision だけ除外する。metadata の `content_hash()` は従来どおり revision も含む。
- `layout_content_hash(&ResolvedText)`、`SceneNodeIr.layout_content_hash`、`CoveragePath.geometry_content_hash` は意味的 key の下流伝播に使う。
- `RenderBackend::execute_with_cache` の既定実装は `execute` を呼ぶ。`CpuReferenceBackend` は `RasterCacheKey` と `RenderCache::rasterize` を使い、実際の path raster だけを再利用する。Group の opacity・合成順・mask・display transform は毎回既存の CPU 参照演算で計算する。
- raster namespace `cpu-reference-f32-v1` は CPU の float32 演算を区別する。GPU backend は今回は texture / raster を保持しない。将来 GPU cache を追加する場合は backend・device・driver の fingerprint を namespace へ固定する。backend 名だけで異なる GPU の画素を共有しない。
- `kronello-eval::DependencyGraph::evaluate_scene_with_properties` は値の取得を純粋な callback として受け、render cache へ逆依存しない。`kronello-text::validate_fonts` は導出 layout 再利用時の明示 byte 照合を提供する。

受け入れ条件の per-level counter、CPU の cached / disabled / direct 実行、cold / warm / 逆順 / eviction / clear、連番ファイル一致の検証は [CACHE-001 の検証](../testing/cache-001.md) を参照。GPU 実機での画素 cache、ディスク永続化、性能目標の実測は今回の保証範囲に含めない。

## RENDER-003 の movie export

[ADR-0079](../adr/0079-bounded-streaming-movie-export.md) の `render_frame_tiles` は
512×512 tile を同期 sink に渡し、sink の処理完了まで次の tile を作らない。
ROI / halo は従来の backwards compiler と絶対画素格子を共有する。
`RenderDag::tile_surface_bytes(16)` は全 image stage の output、Group の children / accumulator、
effect の3 temporaryと root reserveを execution ROI union で数える。
512 MiB超過は `UNSUPPORTED_FEATURE`。backend 固有の安全予算も適用する。
一般 frame / image sequence の最終2面と render.explain の host 面推定は従来のまま。
movie は RGBA8 1面だけを全画面保持し、1 frame ごとに native encoderへ渡す。
音声の spool / bounded Bus、I/O report、実測 RSS と検証範囲は
[RENDER-003](../testing/render-003.md) を参照する。

### M4 temporal executor（RENDER-002 / CACHE-002）

`RenderProfile.temporal` の optional 設定は共通 render.frame / render.sequence / movie export /
固定 worker snapshot に保存する。版 1 は有理数の露光開始位相と midpoint 標本を
root composition scope で共有し、全体合成の線形 premultiplied RGBA を逐次平均する。
Sequence の nominal time を含む視覚編集区間へ露光を切り詰める既定方針と、明示的に跨ぐ
方針を持つ。crossfade は連続な重なりとして扱う。表示変換は平均後の一回だけである。
独立した bounded temporal LRU は snapshot 全内容・全標本依存・requested/execution ROI halo・
backend namespace を key に含む。動画は外部ファイルの検証を省かないため temporal 出力を
保持しない。詳細と制約は [ADR-0080](../adr/0080-root-temporal-integration.md)、
再現手順は [検証記録](../testing/render-002.md) を参照する。

## GPU-003 の strict resident video 経路

[ADR-0081](../adr/0081-guaranteed-metal-hardware-video-decode.md) の
`gpu_resident_bgra8` / `gpu_resident_nv12` は共有 service / CLI / MCP で明示選択する。
framebridge が local compressed demux、VideoToolbox hardware required + query、
IOSurface / same-device Metal import と native ownership を扱う。
GPU shader が BT.709 color conversion / nearest affine sampling を行い、
`ResidentImage` を scene image input として合成する。foreign context は allocation 時の
identity token で拒否し、CPU oracle は resident input を暗黙 readback しない。
最終 linear / display 出力 readback、GPU copy、control upload は
`FrameMetadata.transfer_stats` の actual cumulative counter へ保存する。
`render.explain` は resident selection の estimate と policy notice を返す。
HDR / 10-bit / full-range と CPU temporal accumulation の resident 強制併用は typed unsupported。
形式別の受け入れ実測と未検証範囲は [GPU-003](../testing/gpu-003.md) に記録する。

## M4 HDR と 8K 出力

[ADR-0086](../adr/0086-rec2100-native-precision-and-fixed-hdr-output.md) の optional HDR profile と意味版 1 で、linear Rec.2020 の 1 を 203 cd/m² と定める。PQ/HLG native 10-bit source は RGBA64 を経由して working RGB へ変換する。CPU と明示 GPU upload は共通の working 値を実行する。numeric artifact / HDR movie と SDR display の変換を分離し、SDR movie tone map は独立した明示 profile に限定する。8K は tile ごとの bounded 実行を使い、CPU 全フレーム保持の上限を引き上げない。

## CACHE-003 の実 GPU resource cache

CPU / GPU は同じ版付き `RasterCacheKey` に入力 hash、時刻、ROI / halo、色 / effect 意味版、backend namespace を固定する。GPU node cache は実 RGBA16F texture の容量制限付き LRU、idle pool は独立した entry / byte 容量を持つ。clone / context identity と queue ordering で寿命を守り、公開 preview 面を再利用しない。外部 resident 入力は親の cache hit 前にも検証する。

任意の永続 cache は project 外の OS cache directory に置く。既定 path が HOME 等の広い project 親の内側なら disk のみ無効化し、metadata に判断を残す。明示 project 内 override は typed error。adapter / driver / OS build / shader / 依存版と意味キーが一致する checksum 検証済み面だけを使い、破損は miss、実 I/O failure は `CACHE_IO`。同時 writer は no-replace atomic publication を使う。通常 disk 書込 / hit の readback / upload を実測する。strict GPU-resident と texture preview は disk 往復を使わない。[ADR-0089](../adr/0089-budgeted-gpu-and-external-raster-cache.md)、[検証](../testing/cache-003.md) を参照。

## PERF-001 の単一 GPU graph と観測

最終 linear / display は同じ graph 面から生成し、一度の sticky validation 後に最終出力だけを読み戻す。linear 専用の texture copy は不要。strict resident 動画もこの経路を共有する。`RenderBackend::transfer_stats_total` の入口・出口差分で全 tile / temporal sample を集計し、cache が実行を省略した要求は transfer 0 とする。`GpuContext::allocation_stats` は具体的な renderer 所有 descriptor payload の分類別 live / peak と node peak を返し、idle pool と driver / decoder private memory の未知部分を区別する。[ADR-0092](../adr/0092-single-graph-gpu-final-output-and-observations.md) を参照。


GPU lowering は compiler が末尾に付加した synthetic output root（直前の単一子 `IsolatedComposite`、opacity 1、末尾 `OutputTransform` からの参照）だけを省略する。内部 group の isolation は保つ。最終 SourceOver/store と sticky validation、resident input の事前検証は維持し、GPU cache key 配列も同じ省略に合わせる。保守的 surface admission は省略後の graph に対して行い、512 MiB cap を変更しない。単純 4K preview の限定的な受け入れと複雑 scene の typed failure は別に扱う（[ADR-0092](../adr/0092-single-graph-gpu-final-output-and-observations.md)）。


### 被覆計算の限定的な省略

単色の fill-only outline で有限かつ保守的な境界を求められる場合は、境界外の画素を透明で書き、16点の被覆計算を省く。境界には浮動小数点丸めと画素幅の余裕を加える。stroke、gradient、非有限・極端な座標や scale、境界が不確かな入力は従来のループへ戻る。dispatch の領域は変えず、再利用 surface の全画素を書き直す。被覆のある画素の色変換・sticky validation、scene の容量上限、所有権と意味キーは維持する。

小数座標・異方的 scale・4K相当scale・複数 contour・Evenodd・暗黙の閉路、cache/pool再利用、隠れた数値エラーを旧経路と厳密比較する。実測と適用限界は [GPU検証](../testing/perf-001-gpu-fusion.md) および [PERF-001](../testing/perf-001.md) を参照。
