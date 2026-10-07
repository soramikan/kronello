# TRACK-001 検証

状態: `done`。`kronello-m8-lane-f` の作業ツリーで受け入れた。main への統合・各 OS CI の保証とは区別する。[ADR-0118](../adr/0118-motion-tracking-data-assets.md) に基づく。正本の条件は[バックログ](../backlog/backlog.json)の「点/平面モーショントラッキングの解析を実装する」「結果を Property/DataAsset として参照可能にし式・エフェクトへ接続する」である。

## 受け入れ対応

- 点/平面解析: 新規 crate `kronello-tracking` は固定小数点・決定論的な実装で、`TrackingMode::Points`（1〜8 seeds）のテンプレートマッチングと `TrackingMode::Plane`（厳密に 4 隅）の homography 推定を提供する。`homography4` は 4 点対の DLT を固定 partial pivot の Gaussian elimination で解く。探索は境界付きで、`TRACKING_MAX_FRAMES` = 4000・`TRACKING_MAX_SEEDS` = 8・`TRACKING_MAX_TEMPLATE_RADIUS` = 32・`TRACKING_MAX_SEARCH_RADIUS` = 64 を超える入力は decode 前に型付き拒否する。フレーム時刻は正規化有理数で保持する。
- DataAsset 化: 結果は model の `TrackingDataAsset`（`crates/kronello-model/src/tracking.rs`）として document に保存される。`TrackingSource` は source asset id・stream index・元コンテントハッシュをロックし、`validate` は version・seed 数・半径・frame 数・単調時刻を検査する。`track.analyze`（`crates/kronello-service/src/tracking.rs`）は `audio.analyze` と同じ revision / `idempotency_key` 契約に従い、locked source を `next_rgba` で順次 decode して 1 オブジェクトを atomic に commit する。
- 式・エフェクトへの接続: `TrackingDataAsset::expression_asset` が `ExpressionDataAsset` を導出し、`expression_data.rs` の集約が authored table と同じ `DataAssetCell` 経路へ載せるため、既存の property sampling から tracking 値を参照できる。stale な source は audio analysis と同様に評価全体を型付き失敗させる。

## 確認したテスト

- `cargo test -p kronello-tracking --test tracking --locked`: `point_tracking_follows_translating_block_deterministically`（平行移動ブロックの追跡・決定性）、`plane_tracking_derives_translation_homography`（4 隅から平行移動 homography）、`homography4_solves_exact_known_maps`（既知写像の厳密解）、`tracking_rejects_invalid_and_overbudget_inputs`（seed 数・半径・frame 数・非有限入力の拒否）。
- `cargo test -p kronello-model --test proxy_tracking --locked`: `tracking_asset_hash_table_and_expression_view`（`DataTable` 導出・content hash・expression view）、`legacy_documents_without_new_fields_deserialize`（旧 project の後方互換）。
- `cargo test -p kronello-service --test proxy_tracking --locked`: `track_analyze_persists_revisioned_idempotent_data_asset`（revision pin・idempotency の再実行同一性）、`track_analyze_rejects_bad_requests_before_decoding`（不正 mode・seed・range の事前拒否）。
- `cargo test -p kronello-service --test api --locked`: `track.analyze` の request/response schema・registry・実コマンド実行網羅を含む 12 / 12 PASS。

## 保証範囲外

- スタビライズ（TRACK-002）・オプティカルフロー補間（TRACK-003）は後続タスクで、本記録の対象外。
- マルチスレッド解析・GPU ベースのテンプレートマッチングは実装しない（決定性のため CPU のみ）。
- トラッキング UI からの seeds 対話入力は GUI 側の後続作業。本 lane は共有 API までを保証する。

## この作業時点の実行記録

2026-10-08: 上記テストは全て PASS。`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`（144 suite・0 失敗）を作業ツリーで実行して成功した。`kronello-tracking` crate を新設し workspace / lockfile に登録した。
