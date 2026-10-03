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

GPU-001 の `kronello-gpu` は wgpu 30.0.1 / pollster 1.0.1 を使い、矩形・PAM 素材から線形 premultiplied RGBA16F までの最短経路を実装した。CPU upload / GPU 内コピー / GPU→CPU readback を別の `TransferStats` として記録する。`kronello-framebridge` の unsafe native interop は macOS のモジュール内に隔離し、IOSurface の BGRA8 単一面取り込み・出力を検証する。M0 スパイク自体は通常 renderer / Render DAG を提供しない。M1 の RENDER-001 は下記の節で接続し、他形式の GPU 常駐保証は未実装。実測結果と制約は [スパイク報告](../testing/gpu-spike-m0.md)、基準未登録の golden harness は [比較手順](../testing/golden-comparison.md) を参照。

M1 / Metal の追加実測では、wgpu 30.0.1 の同一 MTLDevice による IOSurface の零コピー import / output と、CVPixelBuffer → CVMetalTextureCache → HAL import が成功した。VideoToolbox の H.264 decode 出力は BGRA8 および NV12 biplanar（R8 / RG8）を取り込めた。H.264 3 frame は両形式で hardware decoder 使用を確認し、BGRA8 のテストパターン最大 channel 誤差は 1。wgpu 出力の VideoToolbox encoder 投入と YCbCr→RGB 精度は未検証。形式ごとの実測値・寿命・同期・転送 counters の範囲は [追加スパイク報告](../testing/gpu-spike-m0.md#corevideo--videotoolbox-追加スパイク) を参照。

## M1 GPU-002 の描画境界

`kronello-gpu::DrawScene` は plain data の `DrawNode` と root のノード参照を受ける。`PathDraw` は出力要求に合わせて flatten・変換済みの `Contour`（design_px）とタグ付き straight `Paint`、`Fill`、`RoundStroke` を保持する。store・フォント探索には依存しない。StrokeJoin / StrokeCap は backend-free なモデル enum を共有する。呼出側が `kronello-vector::flatten` の polyline、または `kronello-text::layout` の positioned outline を flatten した結果を写す。未変換の曲線・モデルの stroke style を暗黙に解釈しない。

### Coverage の定義（vec003-grid4-v2）

- GPU compute で各画素の 4×4 固定サンプルを検査する。画素内位置は `((sx+0.5)/4, (sy+0.5)/4)`、`sx, sy = 0..3`。出力画素から design_px への倍率は `RenderSize` で指定し、被覆率は hit 数 / 16。ハードウェア MSAA のサンプル配置には依存しない。
- fill は Nonzero / Evenodd。開いた contour も fill では暗黙に閉じる。水平辺は winding に寄与せず、Y crossing は半開区間、cross product の符号は厳密な正負で判定する。点が辺上にある場合も同じ判定規約を使う。
- stroke は miter / bevel / round、butt / square / round に対応する。共通の CPU 展開で中央線の矩形・接合三角形・円を生成し、GPU へ同じ領域を渡す。miter limit（既定 4）を越えた接合は bevel。閉じる辺は `Contour.closed` のときだけ stroke に含める。幅 0 は無被覆。幾何定義・退化処理は [04 章の VEC-003 規約](04-vector-text-layout.md#vec-003-の実装規約) を参照。dash・stroke alignment は未対応。
- fill と stroke は別々に coverage を resolve し、stroke を fill の上に source-over する。単色または線形 / 放射 gradient の各 stop は sRGB decode → Rec.709 / Rec.2020 原色変換 → premultiply の順で処理する。各サンプル位置の線形 premultiplied paint を被覆に応じて蓄積・平均する。coverage に伝達関数を適用しない。RGBA8 の色付き raster を経由しない。
- `render_scene_reference` は同じサンプル配置・辺判定・stroke・描画順の CPU 参照。CPU は binary16 丸めを行わない。AA は画素面積の厳密積分ではなく、この版付きサンプリング契約。辺を比較から除外しない。GPU の各中間面は RGBA16F。

### Group、mask、外部出力

`DrawNode::Group` は子の source-over を透明な offscreen RGBA16F へまとめ、opacity を RGB / alpha に一度だけ掛ける。ネストも同じ手順。`DrawNode::Masked` は source と matte の参照を入力とし、matte を表示順に自動挿入しない。共有入力は要求内で一度描画して再利用する。matte 自身を表示したい場合だけ root / children に明示する。

`MaskKind::Alpha` は matte alpha、`MaskKind::Luminance` は作業用線形空間の premultiplied RGB から求めた Y（straight Y × alpha）を `[0,1]` に clamp して coverage にする。Rec.709 は `(0.2126, 0.7152, 0.0722)`、Rec.2020 は `(0.2627, 0.6780, 0.0593)`。alpha を二重に掛けず、encoded sRGB の明度を使わない。色入力を coverage に変える明示ノードであり、既存 coverage を再度色変換するものではない。

`GpuContext::render_scene` は作業用線形 premultiplied `RenderOutput` を返す。`render_scene_output` は `OutputTransform`（sRGB / linear Rec.709 / linear Rec.2020、straight / premultiplied）を必須とし、外部 epsilon に従う unpremultiply → 原色変換 → 必要なら sRGB encode → 指定空間で再 premultiply の順で処理する。`ExternalFrame` に関連付け空間を明示し、encoded premultiplied を内部画像と混同しない。alpha の破棄・背景の推測・HDR の PQ/HLG・tone mapping は提供しない。RGB は clamp せず、各 RGBA16F 面への書き込み前に有限・alpha 範囲・RGB の絶対値 65,504 以下を検証する。GPU は全 pass で共有する sticky status に失敗を記録し、Metal 等が範囲超過値を有限最大値へ飽和させても型付きエラーを返す。後の不透明描画や group opacity で隠れる中間 overflow も拒否する。CPU 参照も各面境界で同じ範囲を検証する。binary16 alpha がゼロへ丸められるときだけ RGB もゼロに正規化し、正の内部 alpha に外部 epsilon を適用しない。

scene は参照欠落・循環・不正 opacity / 色 / 幾何を型付きエラーで拒否する。保守的な予算は 1,024 nodes、各 group / roots 1,024 references、合計 65,536 edges、深さ 32、座標の絶対値・stroke 幅 1,000,000 以下、GPU 中間面の上限推計 512 MiB（CPU 参照にも float32 の面サイズで同じ予算を適用）。デバイス限界超過もエラー。GPU 不在では skip / CPU fallback しない。CPU upload は幾何と制御データのみで、合成・mask は GPU 常駐。`TransferStats` はこれらを control upload として計上し、最終 GPU copy / image readback と、4 bytes の validation status readback（1 回）を計上する。性能・資源プール・tiling・高品質 AA は今後の検証対象。

## M1 RENDER-001 の Scene IR / Render DAG と連番出力

`kronello-render` は store / GPU に通常依存しない。`RenderBackend` を呼出側から渡し、`kronello-gpu::GpuContext` の trait 実装で GPU-002 の coverage・隔離合成・mask・出力変換を実行する。`kronello-gpu::render_adapter::CpuReferenceBackend` は明示選択する float32 の参照実装で、通常の意味テストに使う。GPU adapter 不在を CPU で補う動作はない。render → gpu の参照はテスト用 dev-dependency だけとする（ADR-0043）。既存 GPU API は維持する。

### 固定 snapshot と公開 API

- `RenderSnapshot::new(&Project, CompositionId, revision, RenderProfile)` は文書を複製し、選択した Composition、revision、profile、必要な `FontRef` と意味の版を固定する。`RenderProfile` は作業用線形 Rec.709 / Rec.2020 と flatten tolerance（既定 0.02 output px）。元の Project を編集しても snapshot は変わらない。
- `RenderSnapshot::with_contract` は `SemanticVersions` と `MatteBinding` も明示入力する。公開 snapshot schema は **1**。Serde の strict な envelope を使い、復元時に欠けた版・lock を最新値で補わない。文書意味版・補間版・TimeMap 版・組版版は **1**、vector は `render001-kurbo-flatten-v1`、色は `gpu002-color-v1`、coverage は `vec003-grid4-v2`、stroke geometry は `vec003-centered-stroke-v1`、gradient interpolation は `vec003-linear-premultiplied-pad-v1`。実行時は対応する版と文書の意味版との一致を検証する。
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

Shape の単色 / 線形・放射 gradient fill / stroke と miter / bevel / round join、butt / square / round cap を接続する。stroke の非一様 scale / shear は `UNSUPPORTED_FEATURE`。fill / text の非一様変換は対応する。後続 paint / stroke 機能は opaque で保持し、必要な最終出力は拒否する。glyph ごとに coverage を作り、text の opacity は glyph 全体の合成に一度掛ける。Group / Null / 配置の containment 枠も局所 opacity と順序を保持する。

現行文書型には matte 欄がないため `MatteBinding` を snapshot の明示レンダー入力とする。source / matte とも stable SceneKey。matte は表示 root / children から除外し、`visible = true` の場合だけ表示する。source ごとの binding は一つ、共有 matte の DAG は再利用する。欠落・非アクティブ参照・containment / matte を合わせた循環は失敗する。

scene 1,024 node、DAG 4,096 node、containment / matte recursion 24、出力 16,777,216 pixel の保守的上限を設ける。backend は GPU-002 の 1,024 draw node・32 depth・65,536 edge・512 MiB 面予算をさらに適用し、限界を超えた要求はエラーにする。全画面合成であり、ROI tiling・GPU texture cache・資源 pool・性能保証は未実装。CACHE-001 のインメモリ cache は下記の範囲で実装した。

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

- `schema_version` / `snapshot_schema_version` / `project_schema_version`、`snapshot_content_hash`、元の `revision`（10進文字列）、選択 `composition`。
- `semantic_versions`（document / interpolation / time_map / layout / vector / color / coverage / stroke_geometry / gradient_interpolation）、`font_locks`（family / PostScript 名 / hash / face index）。
- 正規化有理数 `time`（num / den は10進文字列）、連番時の `frame_index`（10進文字列）と `sequence_number`。任意時刻の still では後二項目は null。
- `design_extent`、`region`（origin / extent / pixels）、2×3 の `design_to_pixel`、`working_space`、`flatten_tolerance_px`。
- `numeric` / `display` の各 `ImageFormat`（color_space、transfer_function、alpha、association_space、pixel_format、channel_order、row_order、byte_order、clipping）。
- `backend`（`cpu_reference_float32` または `wgpu_rgba16f`）。厳密 cache の GPU / driver fingerprint 固定は CACHE / QA の後続範囲。

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
