# TRACK-002 検証

状態: `done`。`kronello-m9-lane-c` の作業ツリーで受け入れた。main への統合・各 OS CI の保証とは区別する。[ADR-0122](../adr/0122-stabilization-from-tracking-data.md) に基づく。正本の条件は[バックログ](../backlog/backlog.json)の「トラッキング結果を用いたスタビライズを実装する」「クロップ・補間・境界処理のパラメータを含める」である。

## 受け入れ対応

- トラッキング結果を用いたスタビライズ: 版付きエフェクト `kronello.stabilize` v1（`crates/kronello-model/src/effect.rs` の `EffectParameters::Stabilize`）は TrackingDataAsset 参照・平滑窓・最大変位・最大回転・最大クロップをノード所有 Property として保持する。Scene IR 構築（`crates/kronello-render/src/snapshot.rs` の `resolve_stabilize`）は、解決された source 時刻に対し `kronello_tracking::correction_inverse`（`crates/kronello-tracking/src/stabilize.rs`）の逆補正 `C^-1` を `ResolvedEffect::Stabilize.inverse` へ束縛し、`PixelEffect::Stabilize` の `frame` / `unmap` 行列として `DagNode::Effect` へ lower する。参照先は asset id・stream index・content hash でロックされ、欠落は `TRACKING_DATA_MISSING`、不一致は document 検証の `INVALID_DOCUMENT`（hash）または `TRACKING_DATA_STALE`（stream index）、平滑窓内の全点喪失は `TRACKING_INSUFFICIENT`、crop 超過は `STABILIZE_CROP_EXCEEDED` の型付きエラー。
- クロップ・補間・境界処理のパラメータ: `max_crop`（[0,1] の面積率上限）は上記の検査、境界は `border: fill | replicate | reflect`（既定 `replicate`、`fill` は `fill_color` を使用）、補間は `sampling: nearest | bilinear`。CPU oracle（`crates/kronello-gpu/src/effect.rs`）と WGSL op 13（`crates/kronello-gpu/src/effect.wgsl`）は同一の画素中心座標・境界解決・OOB tap 規約を共有し、binary16 RNE 面境界まで一致する。stabilize の意味版・kernel 版は snapshot `semantic_versions.effects` と raster cache key に含まれ、pin のない snapshot 上の authored stabilize は `UNSUPPORTED_FEATURE`。flow 補間（TRACK-003）との組合せは現版では `UNSUPPORTED_FEATURE` で拒否する。

## 確認したテスト

- `cargo test -p kronello-model --test track002 --locked`: `track002_stabilize_resolves_authored_parameters`（8 Property の解決・enum 変換・inverse 未束縛）、`track002_stabilize_rejects_invalid_enums_and_ranges`（border/sampling の未知値・負値・範囲外の拒否）、`track002_sequence_rejects_interpolation_on_nonvideo`（TRACK-003 組合せの Sequence 検証）。
- `cargo test -p kronello-tracking --test m9_lane_c --locked`: `track002_correction_inverse_is_identity_for_constant_motion`（定速運動 → 恒等補正）、`track002_smoothed_correction_counters_single_frame_spike`（単一フレームのスパイクを平滑化して逆補正）、`track002_stabilize_errors_are_typed`（MissingData / Insufficient / CropExceeded / InvalidInput の code）。
- `cargo test -p kronello-render --test track002 --locked`: `track002_scene_binds_inverse_correction_into_dag`（Scene IR への inverse 束縛と `PixelEffect::Stabilize` への lower、別時刻で異なる補正）、`track002_missing_or_stale_tracking_data_fails_typed`（欠落 `TRACKING_DATA_MISSING`・hash 不一致 `INVALID_DOCUMENT`・stream 不一致 `TRACKING_DATA_STALE`）、`track002_source_time_outside_tracking_range_is_typed`（追跡範囲外の source 時刻）、`track002_unpinned_snapshot_rejects_authored_stabilize`（pin なし snapshot の拒否）。
- `cargo test -p kronello-gpu --test scene track002 --locked`: `cpu_track002_stabilize_borders_and_sampling`（fill / replicate / reflect と nearest / bilinear の境界・端 tap 規約）、`gpu_track002_stabilize_matches_cpu_reference`（WGSL op 13 と CPU oracle の LinearRec709 / LinearRec2020 の pixel一致）。

## 保証範囲外

- stabilize と TRACK-003 frame interpolation の同時適用は v1 では型付き拒否（合成済み中間フレームには安定した追跡格子がないため）。組合せは後続範囲。
- `Plane` mode の homography 行を用いたスタビライズは v1 では points 推定のみ。平面補正は後続範囲。
- マルチスレッド解析・GPU 側の追跡計算は実装しない（TRACK-001 と同じ CPU 決定性方針）。stabilize の warp kernel 自体は GPU で実行する。

## この作業時点の実行記録

2026-10-08: 上記テストは全て PASS。`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` を作業ツリーで実行して成功した（外部フォント fixture `noto-sans-cjk-jp` を要求する 2 件の GPU fixture テストは環境差分として除外し、残りの全 suite は 0 失敗）。
