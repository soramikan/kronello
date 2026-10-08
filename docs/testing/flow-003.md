# FLOW-003 書き出し運用 — 受け入れ記録

状態: `done`（`kronello-m9-lane-f` の作業ツリーで受け入れた。main への統合・他 OS CI の保証とは区別する）。[ADR-0130](../adr/0130-export-presets-batch-and-watch.md) に基づく。正本の条件は[バックログ](../backlog/backlog.json)の「書き出しプリセット・バッチキュー・ウォッチフォルダを実装する」である。

## 受け入れ対応

- 書き出しプリセットは document の `Project.export_presets`（`EXPORT_PRESET_VERSION = 1` の versioned payload）として保存し、`export_preset_save` / `export_preset_delete` の共有 edit command で GUI・CLI・MCP が同一状態を持つ。`composition` / `target` の排他、range・frame_rate・region・profile・output の検証、参照整合は `Project::validate` と model の `ExportPreset::validate` で保証する。destination は提出時にのみ与える。
- `export.batch` は順序付き item を正規化してからキューへ入れる。`submission` / `preset` の排他、preset item の `project` + `destination` 必須、destination 重複拒否、不明 preset の `PRESET_MISSING`、明示 key の重複拒否、未指定 key の `auto:<sha256>` 導出、`stop` / `continue` の deterministic failure policy、item 毎の `submitted` / `replayed` / `skipped` / `failed` 結果を持つ。replay は既存 job を返して新 worker を起こさず、自 job 出力への存在チェックより先に行う。
- `kronello watch --project P --directory D --preset ID|NAME --output DIR [--poll-ms N] [--once]` は poll 型の長時間実行で、lexical path 順の regular file 走査・project file 除外・size/content の連続一致による安定判定を行い、`export.batch` の preset item 経路で提出する。出力名は共有 stem 規則（ASCII 英数字・`-`・`_` 保持、他は `_`、64 文字上限、空は `preset`）+ 共有 `preset_output_extension` の suffix で、GUI バッチと同じ規則を使う。失敗は typed JSON-line で stderr に記録する。
- JobStore に keyed submission（`job_keys` table、schema version 2）を追加し、既存 v1 database の migrate と key の dangling 清掃を確認済み。

## 確認したテスト

- `cargo test -p kronello-service --test flow003 --locked`: `export_presets_roundtrip_through_shared_edits`（保存・reload・validation、`INVALID_MUTATION`）、`export_batch_orders_outcomes_replays_and_reports_item_failures`（順序・idempotent replay・型付き item 失敗・stop/continue）、`export_batch_resolves_stored_presets_server_side`（preset→submission 変換・project path 型付き失敗・`preset_output_extension` 共有規則）。
- `cargo test -p kronello-model --test bins_presets --locked`: `preset_field_validation_is_strict_and_versioned`、`preset_references_validate_against_the_document` 等、version・target 排他・reference の検証。
- `cargo test -p kronello-jobs --test state --locked`: `keyed_submission_replays_rejects_reuse_and_scopes_projects`、`keyed_submission_cleans_up_dangling_keys`、`version_one_database_migrates_to_keyed_schema`。
- `cargo test -p kronello-cli --test watch --locked`: `watch_once_submits_stable_files_through_the_stored_preset`（実 CLI で preset 解決・順序・再実行時の idempotent replay）、`watch_once_marks_failed_items_and_exits_nonzero`（失敗 item の JSON-line と非 0 終了）、`watch_rejects_bad_arguments_and_unknown_presets`、`watch_ignores_the_project_file_inside_the_watched_directory`。
- `swift test --package-path apps/macos --filter MediaFlowTests` の `testPresetsAndBatchQueue`: FakeTransport が `export_preset_save` の payload（version・composition・range・output の mirror）、同名 upsert の安定 id、`export_preset_delete`、document 由来の preset 一覧、`export.batch` の ordered preset item + shared 命名規則 destination、item 失敗の typed 表示を検証した。
- `cargo test -p kronello-service --test api --locked`: `export.batch` / preset command の schema・envelope・registry 網羅を含む。

## 保証範囲外

- `watch` は `--once` で決定的に検証した poll 型。常駐 service ではなく、長時間実運用（数時間）でのメモリ・fd・ログ量は未計測。
- watch の安定判定は size/content の連続一致ベースで、同一 poll 中に書き込みが完了する tiny file 競合は検出不能なことがある（実装は繰り返し観測で緩和するのみ）。
- batch job の実 render 実行は既存 job infra に従属する。`export.batch` の検証は提出・順序・idempotency・型付き失敗まで。

## この作業時点の実行記録

2026-10-08: `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`（162 suite・0 失敗、`watch` 4 tests 含む）を作業ツリーで実行して成功した。`python3 scripts/build_ffi.py` で FFI / CLI を構築し、`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --package-path apps/macos` と `swift test --package-path apps/macos`（113 tests・0 失敗・1 skip）を成功させた。`MediaFlowTests` は export preset / batch の wire 形を FakeTransport で確認した。GUI の実バッチ書き出し・watch の実運用は本記録の検証範囲外。
