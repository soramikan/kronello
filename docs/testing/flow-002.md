# FLOW-002 メディア管理 UI — 受け入れ記録

状態: `done`（`kronello-m9-lane-f` の作業ツリーで受け入れた。main への統合・他 OS CI の保証とは区別する）。[ADR-0129](../adr/0129-media-bins-and-offline-management.md) に基づく。正本の条件は[バックログ](../backlog/backlog.json)の「ビン・メディアブラウザ・サムネイル一覧を実装する」「オフラインメディアの表示と relink 操作を接続する」である。

## 受け入れ対応

- ビンは document の `Project.bins`（`id` / `name` / `assets`、重複許可の安定 ID 参照）として保存され、`bin.create` / `bin.rename` / `bin.delete` / `bin.assign` の共有 edit command で GUI・CLI・MCP が同一操作を行う。store の diff は `bins` を unordered collection として stable id で比較し、順序変化で無関係のメンバーを置き換えない。
- メディアブラウザは共有 query の `media.query` が asset の kind・stream・locator・`present_unverified` / `missing` の安価な存在 probe と typed error を返す。GUI のメディアページ（⌘5）はアセット grid・bin 一覧・検索・bin 所属のコンテキストメニュー・詳細ペインを持つ。
- サムネイルは `asset.thumbnail`（固定 snapshot の `VideoDecoder::rgba_at` + 決定的 `scale_rgba8`、16..=1024 px 上限）が opaque RGBA8 を返す。ピクセルは transport データであり GUI は in-memory cache にだけ保持し、document へは保存しない。
- オフラインメディアは probe 結果でバッジ表示し、`asset.relink`（`base_revision` 付き、検証済み search directory）を GUI のボタンとコンテキストメニューへ接続した。relink 成功後は document reload と media.query 再取得で状態が更新される。

## 確認したテスト

- `cargo test -p kronello-service --test flow002 --locked`: `bins_edit_through_shared_commands_persist_and_media_query_reports`（共有 command の bin 作成・rename・assign・delete、保存・再読込、media.query の availability と typed error）、`asset_thumbnail_decodes_png_deterministically_and_fails_typed`（PNG の決定的 RGBA、size 0 / 15 / 1025 の `INVALID_REQUEST`、asset 欠落の `ASSET_MISSING`）。
- `cargo test -p kronello-model --test bins_presets --locked`: bin の空名・重複 member 拒否、asset 参照検証、opaque asset の member 許可、空 collection の省略 round-trip。
- `cargo test -p kronello-service --lib --locked` の `edit::regression_tests::unordered_collections_keep_member_patches_for_selective_undo`: bins の reorder で patch が member 単位になり選択 Undo が保持される。
- `cargo test -p kronello-media --test media --locked` の `rgba_at_returns_deterministic_opaque_sdr_frame`: 任意時刻の RGBA8、opaque alpha、seek 後・fresh decoder との一致、範囲外時刻の `FRAME_NOT_FOUND`。
- `swift test --package-path apps/macos --filter MediaFlowTests` の `testMediaBrowserBinsAndOffline` / `testThumbnailCacheAndFailures`: FakeTransport が `media.query` / `asset.thumbnail` / `asset.relink` / `bin_*` command の wire 形を検証し、offline バッジ・bin 絞り込み・membership 全体置換・thumbnail cache の重複要求抑止を確認した。
- `cargo test -p kronello-service --test api --locked` の `every_request_payload_and_envelope_matches_schema_and_denies_execution_fields` 等: 新 operation の schema / registry 網羅。

## 保証範囲外

- 実機 GUI での drag & drop 取り込み、thumbnail の表示品質、大量アセット時のスクロール性能は未計測。`MediaFlowTests` は wire 形と cache 規約を検証し、ピクセルの目視確認は行っていない。
- `asset.thumbnail` は image / video のみ。audio asset には `UNSUPPORTED_FEATURE`（GUI は kind アイコン表示にフォールバック）。

## この作業時点の実行記録

2026-10-08: `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`（162 suite・0 失敗）を作業ツリーで実行して成功した。`KRONELLO_SCHEMA_UPDATE=1` で `schemas/project-v1.schema.json` / `schemas/api-v1.schema.json` を再生成し、`python3 scripts/generate_swift_api.py --check` が差分なし。`python3 scripts/build_ffi.py` で cdylib / CLI を構築し、`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --package-path apps/macos` と `swift test --package-path apps/macos`（113 tests・0 失敗・1 skip は `KRONELLO_INTEGRATION_EVIDENCE` 依存の既存 opt-in）を成功させた。CommandLineTools 単体では SwiftUI macro plugin を解決できないため全 Xcode を使った。
