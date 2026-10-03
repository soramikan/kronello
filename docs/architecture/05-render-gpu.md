# 05 レンダラーと GPU

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

M2 で drop shadow と gaussian blur を実装する（FX-001）。エフェクトは必要な入力領域（ROI の halo）を宣言し、結果は visual_bounds に反映する。エフェクトのパラメーターは Property 基盤に乗せる。

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

GPU-001 の `kronello-gpu` は wgpu 30.0.1 / pollster 1.0.1 を使い、矩形・PAM 素材から線形 premultiplied RGBA16F までの最短経路を実装した。CPU upload / GPU 内コピー / GPU→CPU readback を別の `TransferStats` として記録する。`kronello-framebridge` の unsafe native interop は macOS のモジュール内に隔離し、IOSurface の BGRA8 単一面取り込み・出力を検証する。通常 renderer / Render DAG / 他形式の GPU 常駐保証は未実装。実測結果と制約は [スパイク報告](../testing/gpu-spike-m0.md)、基準未登録の golden harness は [比較手順](../testing/golden-comparison.md) を参照。

M1 / Metal の追加実測では、wgpu 30.0.1 の同一 MTLDevice による IOSurface の零コピー import / output と、CVPixelBuffer → CVMetalTextureCache → HAL import が成功した。VideoToolbox の H.264 decode 出力は BGRA8 および NV12 biplanar（R8 / RG8）を取り込めた。H.264 3 frame は両形式で hardware decoder 使用を確認し、BGRA8 のテストパターン最大 channel 誤差は 1。wgpu 出力の VideoToolbox encoder 投入と YCbCr→RGB 精度は未検証。形式ごとの実測値・寿命・同期・転送 counters の範囲は [追加スパイク報告](../testing/gpu-spike-m0.md#corevideo--videotoolbox-追加スパイク) を参照。

## M1 GPU-002 の描画境界

`kronello-gpu::DrawScene` は plain data の `DrawNode` と root のノード参照を受ける。`PathDraw` は出力要求に合わせて flatten・変換済みの `Contour`（design_px）とタグ付き straight `Paint`、`Fill`、`RoundStroke` を保持する。文書モデル・store・フォント探索には依存しない。呼出側が `kronello-vector::flatten` の polyline、または `kronello-text::layout` の positioned outline を flatten した結果を写す。未変換の曲線・モデルの stroke style を暗黙に解釈しない。

### Coverage の定義（gpu002-grid4-v1）

- GPU compute で各画素の 4×4 固定サンプルを検査する。画素内位置は `((sx+0.5)/4, (sy+0.5)/4)`、`sx, sy = 0..3`。出力画素から design_px への倍率は `RenderSize` で指定し、被覆率は hit 数 / 16。ハードウェア MSAA のサンプル配置には依存しない。
- fill は Nonzero / Evenodd。開いた contour も fill では暗黙に閉じる。水平辺は winding に寄与せず、Y crossing は半開区間、cross product の符号は厳密な正負で判定する。点が辺上にある場合も同じ判定規約を使う。
- 基本 stroke は幅 / 2 を半径とする線分 capsule の和集合（round cap / round join）。距離が半径に一致すれば hit。閉じる辺は `Contour.closed` のときだけ stroke に含める。幅 0 は無被覆。miter / bevel / butt / square / dash はこの API の対応 style ではなく、上位で対応形状へ展開するか未対応エラーにする。
- fill と stroke は別々に coverage を resolve し、stroke を fill の上に source-over する。coverage のヒット処理と色変換を分離し、色は sRGB decode → Rec.709 / Rec.2020 原色変換 → premultiply → coverage の順に処理する。coverage に伝達関数を適用しない。RGBA8 の色付き raster を経由しない。
- `render_scene_reference` は同じサンプル配置・辺判定・stroke・描画順の CPU 参照。CPU は binary16 丸めを行わない。AA は画素面積の厳密積分ではなく、この版付きサンプリング契約。辺を比較から除外しない。GPU の各中間面は RGBA16F。

### Group、mask、外部出力

`DrawNode::Group` は子の source-over を透明な offscreen RGBA16F へまとめ、opacity を RGB / alpha に一度だけ掛ける。ネストも同じ手順。`DrawNode::Masked` は source と matte の参照を入力とし、matte を表示順に自動挿入しない。共有入力は要求内で一度描画して再利用する。matte 自身を表示したい場合だけ root / children に明示する。

`MaskKind::Alpha` は matte alpha、`MaskKind::Luminance` は作業用線形空間の premultiplied RGB から求めた Y（straight Y × alpha）を `[0,1]` に clamp して coverage にする。Rec.709 は `(0.2126, 0.7152, 0.0722)`、Rec.2020 は `(0.2627, 0.6780, 0.0593)`。alpha を二重に掛けず、encoded sRGB の明度を使わない。色入力を coverage に変える明示ノードであり、既存 coverage を再度色変換するものではない。

`GpuContext::render_scene` は作業用線形 premultiplied `RenderOutput` を返す。`render_scene_output` は `OutputTransform`（sRGB / linear Rec.709 / linear Rec.2020、straight / premultiplied）を必須とし、外部 epsilon に従う unpremultiply → 原色変換 → 必要なら sRGB encode → 指定空間で再 premultiply の順で処理する。`ExternalFrame` に関連付け空間を明示し、encoded premultiplied を内部画像と混同しない。alpha の破棄・背景の推測・HDR の PQ/HLG・tone mapping は提供しない。RGB は clamp せず、各 RGBA16F 面への書き込み前に有限・alpha 範囲・RGB の絶対値 65,504 以下を検証する。GPU は全 pass で共有する sticky status に失敗を記録し、Metal 等が範囲超過値を有限最大値へ飽和させても型付きエラーを返す。後の不透明描画や group opacity で隠れる中間 overflow も拒否する。CPU 参照も各面境界で同じ範囲を検証する。binary16 alpha がゼロへ丸められるときだけ RGB もゼロに正規化し、正の内部 alpha に外部 epsilon を適用しない。

scene は参照欠落・循環・不正 opacity / 色 / 幾何を型付きエラーで拒否する。保守的な予算は 1,024 nodes、各 group / roots 1,024 references、合計 65,536 edges、深さ 32、座標の絶対値・stroke 幅 1,000,000 以下、GPU 中間面の上限推計 512 MiB（CPU 参照にも float32 の面サイズで同じ予算を適用）。デバイス限界超過もエラー。GPU 不在では skip / CPU fallback しない。CPU upload は幾何と制御データのみで、合成・mask は GPU 常駐。`TransferStats` はこれらを control upload として計上し、最終 GPU copy / image readback と、4 bytes の validation status readback（1 回）を計上する。性能・資源プール・tiling・高品質 AA は今後の検証対象。
