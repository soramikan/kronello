# TRACK-003 検証

状態: `done`。`kronello-m9-lane-c` の作業ツリーで受け入れた。main への統合・各 OS CI の保証とは区別する。[ADR-0123](../adr/0123-optical-flow-retime-interpolation.md) に基づく。正本の条件は[バックログ](../backlog/backlog.json)の「オプティカルフローによる中間フレーム補間を実装する」「リタイムとの組み合わせと失敗時の型付き拒否を検証する」である。

## 受け入れ対応

- オプティカルフロー中間フレーム補間: `PiecewiseTimeMap` の版付き `interpolation` モード `"optical_flow"`（`OpticalFlowConfig`: block/search 半径・levels・有理数 `confidence_floor`・`max_low_confidence`・`flow_fallback`）が Scene IR の `SceneContent::Video.interpolation` → `DagNode::VideoDraw.interpolation` → `decode_video_image_with_sampling` へ運ばれる。mapped source 時刻が presentation 中間のとき、両隣の decode 済み frame を luma 化して `kronello_tracking::estimate_flow`（前後両方向）→ `consistency_combine` → `confidence_gate` → `interpolate_frames` の順で双方向 warp 合成する。pts 上の時刻・末尾 frame では flow を推定せず decode 済み frame をそのまま返す。flow は必要な frame 対だけ遅延計算し、外部プロセス・時計・非固定乱数を使わない。
- リタイムとの組み合わせと失敗時の型付き拒否: `Sequence::validate` は interpolation を video asset clip・順方向サンプリングに限定し、非 video・`Linear`/`Protected` map への付与は `Unsupported` で拒否する。decode 層は HDR・`reverse_sampling`・image asset を `UNSUPPORTED_FEATURE`、低信頼領域の割合超過を `FLOW_CONFIDENCE_LOW` の型付きエラーとし、crossfade は authored `flow_fallback: "blend"` のみで `blend_frames` へ明示 downgrade する。resident decode 経路は flow を明示拒否する（明示 software 経路を要求）。snapshot は `semantic_versions.frame_interpolation = 1` の pin を要求し、pin のない旧 snapshot は authored interpolation を持つ project で `UNSUPPORTED_FEATURE`。補間 mode は raster cache key（`resident-video-raster` identity）に含まれる。

## 確認したテスト

- `cargo test -p kronello-time --test contracts --locked`: `track003_frame_interpolation_wire_and_validation`（wire 形式・config 検証・map 同一性への mode 含有）。
- `cargo test -p kronello-tracking --test m9_lane_c --locked`: `track003_flow_recovers_integer_translation_and_is_deterministic`（整数平行移動の回復と決定性）、`track003_confidence_gate_rejects_decorrelated_and_fallback_blends`（無相関入力の `FLOW_CONFIDENCE_LOW` と authored blend）、`track003_interpolate_places_block_at_the_fraction`（fraction に応じた block 位置の合成）。
- `cargo test -p kronello-media --test track003 --locked`: `track003_optical_flow_synthesizes_mid_interval_frame`（ProRes 2 frame 間の中間合成と pts 上の skip）、`track003_low_confidence_rejects_without_authored_fallback`、`track003_authored_blend_fallback_downgrades_to_crossfade`、`track003_reverse_sampling_rejects_interpolation`、`track003_snapshot_binds_interpolation_and_renders_synthesis`（composition → snapshot → scene → render の縦断と `VideoDraw.interpolation` の伝達）、`track003_unpinned_snapshot_rejects_authored_interpolation`（pin なし snapshot の拒否と非 authored 時の継続有効）、`track003_stabilize_on_interpolated_clip_is_rejected`（TRACK-002 との組合せ拒否）。
- `cargo test -p kronello-model --test track002 --locked`: `track002_sequence_rejects_interpolation_on_nonvideo`（非 video clip・逆方向への interpolation 付与の Sequence 検証拒否）。

## 保証範囲外

- HDR・逆方向サンプリング・image asset・stabilize との組合せ・resident GPU decode 経路は v1 では型付き拒否であり、黙って decode を続行しない。各対応は後続タスクの範囲。
- flow 推定は CPU の決定的 block matching で、GPU 側の推定・マルチスレッド化は実装しない。
- 大域的な事前 bake は行わず、必要 frame 対のみ遅延計算する（[ADR-0123](../adr/0123-optical-flow-retime-interpolation.md)）。

## この作業時点の実行記録

2026-10-08: 上記テストは全て PASS。`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` を作業ツリーで実行して成功した（外部フォント fixture `noto-sans-cjk-jp` を要求する 2 件の GPU fixture テストは環境差分として除外し、残りの全 suite は 0 失敗）。
