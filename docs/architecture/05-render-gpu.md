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

OpenFX の入力領域 / 必要フレームの問い合わせに似た契約を参考にするが、OpenFX ホスト互換をこの段階で約束しない。

## 合成

- 内部の色付き画像は、明示した作業用線形空間と premultiplied alpha を基本とする。既定の作業用色空間は未決（[OQ-11](../open-questions.md)）。
- Group の既定は、子を一度まとめてから Group opacity / 効果を適用する isolated 方式。
- 各子に opacity を配る最適化は意味が一致する場合だけ行う。
- Blend mode が表示基準の色空間を必要とする場合は明示変換を置き、すべて線形で同じ見た目になるとしない。
- 外部出力の straight / premultiplied alpha 変換、ゼロ alpha 付近、マット境界を検証する。

## 基本エフェクト

M2 で drop shadow と gaussian blur を実装する（FX-001）。エフェクトは必要な入力領域（ROI の halo）を宣言し、結果は visual_bounds に反映する。エフェクトのパラメーターは Property 基盤に乗せる。

## 高解像度

RGBA16F の 3840x2160 は 63.28125 MiB、7680x4320 は 253.125 MiB（画像データのみ）。

- デコード面、参照フレーム、中間テクスチャ、字形アトラス、蓄積バッファ、エンコーダーを別に予算化する。
- macOS の共有メモリを独立した VRAM と同じ予算計算にしない。
- CPU / GPU 往復、GPU 内コピー、FrameBridge の同期待ちを計測する（[ADR-0008](../adr/0008-explicit-cpu-gpu-transfer-paths.md)）。
- Vulkan / Metal / D3D12 との相互運用は専用モジュール（`koma-framebridge`）に隔離し、参照デバイス・所有権・同期の契約をテストする。

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

レンダーキャッシュは作品データではない。`.koma` の外（OS のキャッシュ領域）に置き、削除しても作品を失わない（[ADR-0006](../adr/0006-document-vs-render-cache.md)）。
