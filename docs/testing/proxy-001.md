# PROXY-001 検証

状態: `done`。`kronello-m8-lane-f` の作業ツリーで受け入れた。main への統合・各 OS CI の保証とは区別する。[ADR-0119](../adr/0119-proxy-workflow.md) に基づく。正本の条件は[バックログ](../backlog/backlog.json)の「プロキシ生成 job と編集時のプロキシ参照切替を実装する」「書き出しでは元素材を用いることを検証する」である。

## 受け入れ対応

- プロキシ生成 job: `proxy.generate` は対象 asset・ストリーム・scale を検証し、固定入力 `ProxyJobInput`（project パス・document hash・asset 複製・destination・寸法・scale）を持つ job を `JobStore` へ登録して分離 worker を起動する（`crates/kronello-service/src/proxy.rs`）。worker は `execute_proxy_job` で `crates/kronello-media/src/proxy.rs` の `encode_proxy` を実行する。出力は `<project>.proxies/<asset-id>.mov` の ProRes で、ジョブ staging から `persist_noclobber` の原子的 rename で公開し、probe で codec・寸法・stream index・start time 0 を確認したうえで SHA-256 コンテントハッシュを asset へ記録する。proxy は通常の `AssetKind::Video` asset として登録され、`ProxyLink`（original/proxy asset・双方の stream index・scale・寸法・元ハッシュ・生成元 job id）が document に保存される。プロキシファイル削除済み job の resume / reconciliation は receipt と成果物同一性を再検証する。
- 編集時の切替: オーサリング上の Media ノードとシーケンスクリップは常に original asset id を保持する。`media_proxies: "prefer"` は `render.frame` / `render.range` 等の RenderInput にある一時的な選択で、共有 Command/Query API・CLI・生成済み Swift API に同一の形で露出する。`RenderSnapshot::with_media_proxies` で snapshot へ載せ、`crates/kronello-render/src/snapshot.rs` の `render_media` が有効な `ProxyLink` のみ decode を proxy asset/stream へ差し替える。`kronello-service/src/lib.rs` の `prune_unresolvable_proxy_links` が prefer 時に locate・ハッシュ検証を通らない link を transient document から落とすため、proxy ファイル欠落・改変・stale link は original へのフォールバックとなり、保存済み document は変更しない。
- 書き出しは元素材: `MediaProxyMode::Off` 以外を `AvExportSnapshot::validate`（`crates/kronello-media/src/export.rs`）と render job 入力（`crates/kronello-service/src/jobs.rs` の 2 箇所）が `UNSUPPORTED_FEATURE` / 型付き拒否で弾き、CLI の file 書き出し経路でも `media_proxies` を拒否する。export には proxy 置換経路が存在しない。
- relink / collect / status / clear: `proxy.status` は link の metadata 状態（ready / missing / stale）を報告し、`proxy.clear` は link を除去して参照のなくなった managed proxy asset object だけを document から外す（ファイルは消さない）。`asset_in_use` は composition node・sequence clip・template instance・repeater binding を走査する。proxy asset は通常 asset なので既存の relink / collect 経路にそのまま乗り、resolve 時の完全 SHA-256 検証により差し替えられた bytes は受理しない。

## 確認したテスト

- `cargo test -p kronello-cli --test proxy --locked`: `proxy_job_registers_link_and_preview_substitutes_it`。実 CLI と実 worker subprocess で `proxy generate` → job 成功 → `<project>.proxies/*.mov` 公開 → asset の content hash が公開ファイルと一致 → `proxy status` が ready を返す → 1px チェッカーボード原素材に対し `prefer` 描画が半解像度 proxy の隣接等値画素を示し `off` 描画が原画を保持 → proxy ファイル削除で `status` が missing・`prefer` が原画へフォールバック → `proxy clear` で link 除去後も原画を描画、までを縦断する。
- `cargo test -p kronello-service --test proxy_tracking --locked`: `proxy_generate_submits_fixed_input_jobs_and_collect_relinks`（job 登録・固定入力・公開後の relink/collect 保持）、`proxy_status_and_clear_manage_link_state`、`file_writing_renders_reject_preview_proxy_mode`（prefer を載せた file 出力の型付き拒否）。
- `cargo test -p kronello-media --test proxy --locked`: `encode_proxy_produces_verified_half_scale_prores`（半分・偶数寸法の ProRes 出力・probe 検証・ハッシュ）、`scale_rgba8_is_deterministic_and_opaque`（固定小数点スケーラの決定性）、`export_snapshot_rejects_preview_proxy_mode`。
- `cargo test -p kronello-render --test proxy --locked`: `prefer_mode_substitutes_a_valid_registered_proxy`、`stale_missing_or_stream_mismatched_links_fall_back_to_original`（stream index 不一致・壊れた link・対象 asset 欠落で authored stream へ戻る）。
- `cargo test -p kronello-model --test proxy_tracking --locked`: `proxy_dimensions_are_even_bounded_and_sane`、`proxy_link_validation_and_document_state`、`asset_in_use_covers_media_nodes_clips_and_bindings`、`legacy_documents_without_new_fields_deserialize`。
- `cargo test -p kronello-service --test api --locked`: 12 / 12 PASS。`proxy.generate` / `proxy.status` / `proxy.clear` の request/response schema、registry・capabilities・envelope・実コマンド実行網羅を含む。

## 保証範囲外

- macOS GUI の proxy 切替 UI 配線は本 lane の対象外。切替は共有 API の `media_proxies` 入力として実装済みで、GUI からの利用は同じ経路を使う。
- proxy 再生成（再エンコード）の UI / 自動起動、original 側のコンテンツハッシュ変化検出による自動 stale 化の運用フローは未実装（`proxy.status` の stale 報告で検出可能）。
- GPU 画素の固定環境 golden 比較、実機 GUI 受け入れは対象外。

## この作業時点の実行記録

2026-10-08: 上記テストは全て PASS。`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`（144 suite・0 失敗）を作業ツリーで実行して成功した。`KRONELLO_SCHEMA_UPDATE=1` で `schemas/project-v1.schema.json` / `schemas/api-v1.schema.json` を再生成し、`scripts/generate_swift_api.py` で `GeneratedAPI.swift` を更新した。`generate_swift_api.py --check` は差分なし、`swiftc -typecheck` で KronelloCore（GeneratedAPI.swift を含む）がコンパイルすることを確認した。GUI ターゲットを含む全体の `swift build` は本環境（CommandLineTools のみで SwiftUI macro plugin を解決できない）では完了しない既知の制約であり、全 Xcode ホストでの検証に委ねる。
