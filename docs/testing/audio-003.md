# AUDIO-003: Sequence / Composition 音声・clip volume の検証

設計: [ADR-0063](../adr/0063-document-audio-and-clip-volume.md)。
2026-10-05、branch `m3-audio3`、base HEAD `38f205058bcf7786c276de3d0e67b73d0869e969` に対する
本 worktree の変更。Codex は GPU / hardware codec capability を持たない。CPU reference と
native FFmpeg software ProRes / PCM24 を使う。host の結果をここへ転記していない。

## 受け入れ条件と対応する証拠

| 条件 | tests / 再現手順 | 確認範囲 |
|---|---|---|
| 1. 同じ固定入力から Sequence 音声と同期 / job MOV | CLI jobs `document_audio_sync_and_fixed_job_survive_volume_edit_and_project_deletion_ntsc` | 実 `render.export` / `render.submit`、Sequence の video + audio tracks、ProRes / PCM24、report の両 hash と frames、decoded video 全3 frames / PCM 全 samples の一致 |
| 2. explicit と document の選択・二重加算防止 | media audio `document_audio_source_modes_are_explicit_backward_compatible_and_hashed`、既存 CLI `sequence_target_job_preserves_placements_after_trim_and_project_removal` | legacy schema 1 / 省略 mode の explicit、空 clips の silence、version 2 の document / silence、非空 clips 併用の型付き拒否、mode で export hash が変わる |
| 3. 共有 volume API、純粋 animation、編集 / hash | service nle `clip_volume_edit_is_revisioned_idempotent_undoable_and_changes_fixed_hash`、audio document `volume_curves_use_source_time_and_reject_negative_evaluated_gain` | `edit.plan/apply` の ClipSetVolume、revision 1→2→3、同一 retry の event ID、Undo の厳密復元、RenderSnapshot / export hash、Curve sample の local time、負 Gain の拒否、compile 後の Curve 編集からの独立性 |
| 4. recursive mapping、配置独立性、trim / 負時刻 / NTSC | audio document `nested_placements_trim_negative_grid_and_request_order_are_independent` / `fractional_asset_trim_keeps_affine_sample_phase`、既存 audio mixing sample-grid tests、CLI NTSC test | 同一 definition の2 Instance、負 Sequence placement、祖先 active 区間、Clip×Media Gain、逆順の分割要求と一括の一致、fractional Asset trim の sample phase、30000/1001 の3 frames = 4804 samples、probe.verify_av の duration 差 < 1/48000秒 |
| 5. 投入後編集 / 削除、欠落 / hash / clipping と非公開 | CLI jobs `document_audio_worker_missing_hash_mismatch_and_clipping_never_publish` と上記 fixed-job test | gate 付き実 worker の後で volume / visual 編集、作品削除、decoded A/V の維持。外部素材削除 / 改変 / 過大 Gain で failed + ASSET_MISSING / ASSET_HASH_MISMATCH / AUDIO_CLIPPING、destination が存在しない |
| AUDIO-004 / COMP-002 の境界 | audio document `retime_effect_generator_missing_assets_and_recursive_audio_fail_typed`、media modes test の Video Media export | unity 以外の map、effects、Generator、循環、欠落を型付き拒否。Video Media は UNSUPPORTED_FEATURE、MOV を公開しない |
| 公開 schema / adapter | service `public_schemas_match_rust_generators` / API の全 command request / response tests、CLI jobs の各 wire response、MCP stdio suite、Swift generator --check | Rust 型・JSON schema・registry・GeneratedAPI.swift の一致。render.export の schema / response decode。GUI Inspector の実装や GUI の実機動作は範囲外 |

各 tests は実行結果の表と併せて読む。既存 test は今回新規に実装したものとして数えない。
M2 explicit の rounding 規則は既存 `source_trim_and_placement_boundaries_use_absolute_floor` で維持する。
文書音声は affine offset の逆写像の一回 floor により rational trim で phase を保持する。先頭の source sample が
負なら AUDIO_SOURCE_TOO_SHORT とし、silence / clamp へ代替しない。

## 公開変更

- model: Clip.volume（optional Property）、NodeKind::Media / MediaNode、descriptor
  `kronello.audio.volume`（UUID `e9cf4a80-2b64-4b8e-9e29-dfe6bc119a63`）。Scalar / dimensionless、
  `[0,f32::MAX]`、Constant / Curve、Expression / Modifier は不可。
- edit: `{"timeline":{"clip_set_volume":{"sequence":"UUID","clip":"UUID","volume":Property-or-null}}}`。
  Sequence / Clip の構造 changed keys を使う。独立した入口専用編集状態は作らない。
- output: ProResMov.audio（document / explicit / silence、既定 explicit）、profile_version（既定1、
  document / silence は2を要求）。同期 `render.export` と ResultData.movie。
- media: AvExportSnapshot.with_audio、schema 2 の mode / compiled placements、report の
  audio_source / audio_profile_version。schema 1 と旧 report の省略値 explicit / 1 を維持する。
- audio: DocumentAudioPlan / AudioTarget / AudioSourceMode、mix_with_gain。plan は必要な Property / Curve を所有する。
- capabilities.features: document_audio / clip_volume / media_audio。
- schemas/project-v1.schema.json / api-v1.schema.json と
  apps/macos/Sources/KronelloCore/GeneratedAPI.swift を再生成。外枠の schema 1 を不用意に増版しない。

`UNSUPPORTED_FEATURE`、`INVALID_AUDIO_INPUT`、`INVALID_MEDIA_INPUT` と既存
`SOURCE_MISSING` / `ASSET_MISSING` / `ASSET_HASH_MISMATCH` / `AUDIO_SOURCE_TOO_SHORT` /
`AUDIO_CLIPPING` / `AUDIO_OVERFLOW` / `TIME_ERROR` を使い、fallback で成功にしない。

## 再現環境と command

worktree root で共有 CARGO_HOME / CARGO_TARGET_DIR と管理 TMPDIR を使用する。
`CARGO_BUILD_JOBS=3`。今回の target は
`ecb82f23c7feedec955928d59a71318b8e7f6361ecae081c3526e0be3364ca74`、scratch は
`worker_1d0924e2f7a34c7eb232c3c35a731a39`。Cargo.lock に animation / schemars と test 依存の
既存 package を追加しただけで、追加 download や per-job cache は作っていない。

```sh
export CARGO_BUILD_JOBS=3
export KRONELLO_STATE_ROOT="$TMPDIR/audio003-state"
KRONELLO_SCHEMA_UPDATE=1 KRONELLO_UPDATE_API_SCHEMA=1 cargo test -p kronello-service --test nle_schema --locked
KRONELLO_UPDATE_API_SCHEMA=1 cargo test -p kronello-service --test api public_schema_matches_and_all_registry_schemas_are_safe --locked
python3 scripts/generate_swift_api.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_
python3 scripts/generate_swift_api.py --check
```

nle_schema の既存更新 flag は KRONELLO_SCHEMA_UPDATE なので、指定された
KRONELLO_UPDATE_API_SCHEMA と併記して Project / API の両 schema を生成した。
API 専用 test も指定 flag で実行した。

## 実行結果

最終 workspace run は **475 passed / 0 failed / 1 ignored / 7 filtered**（75 suite result lines）。
GPU / framebridge crates の test 実行は command で除外した。新規 AUDIO-003 の8 tests はすべて成功した。
ignored は既存 store `snapshot_policy_evaluation`（明示実行する policy measurement）。
`--skip gpu_` は substring filter なので、次の7 tests を除いた。フィルタされた結果を成功に含めない。

- CLI: gpu_headless_default_backend_animated_shape_japanese_text_sequence
- MCP: default_gpu_failure_never_falls_back_and_explicit_cpu_selection_works
- render: gpu_animated_shape_and_japanese_text_match_cpu_all_pixels_and_order
- render: gpu_isolated_alpha_and_luma_mattes_and_sequence_match_reference
- render: gpu_gradient_dag_transform_and_animation_match_cpu_reference
- render: gpu_fx_cropped_roi_matches_full_render_with_stacked_effects
- service: evaluated_template_queries_share_layout_inputs_and_do_not_initialize_gpu_or_edit

| 実行 command | exit / 結果 |
|---|---|
| cargo fmt --all --check | 0、整形差分なし |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 0、警告なし（最終 log: audio003-clippy-verified.log） |
| cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_ | 0、475 / 0 / 1 / 7。CARGO_BUILD_JOBS=3 と管理された KRONELLO_STATE_ROOT を設定（最終 log: audio003-workspace-tests-state.log） |
| python3 scripts/generate_swift_api.py --check | 0、生成物一致 |
| 両 flag を設定した service nle_schema | 0、1 passed。Project / API schema を実際に再生成 |
| KRONELLO_UPDATE_API_SCHEMA=1 の service API schema test | 0、1 passed（他10 filtered）、API schema を再生成 |
| python3 scripts/generate_swift_api.py | 0、GeneratedAPI.swift を実際に更新 |
| python3 scripts/fixtures.py generate | 0、9 media fixtures を生成・decode |
| python3 scripts/fixtures.py check | 0、16 entries / 9 scenes / bundled 28012 bytes を検証 |
| git diff --check | 0、whitespace 問題なし |

個別の開発中 checks も実行した。cargo check -p kronello-service --locked、audio 全 suite、
service の clip_volume、CLI jobs の document_audio 2件、media audio の document_audio 1件は成功。
型・fixture の初期エラーを修正してから最終 workspace / Clippy で全変更を再確認した。

最初の workspace run は CLI machine で10 passed / 8 failed / 1 filtered。
7 failures は Noto fixture 未配置、1 failure は今回追加した render.export に対する
旧 registry count 32 の期待値。既存 font を repository 本体の target/fixtures/external から
この worktree の同じ target path へコピーし、manifest check で検証した。期待値を33へ更新した。
次の run は FFI boundary の7 passed / 1 failed。既存 subscription test の job.list が
sandbox 外の既定 state root に書こうとしたため、KRONELLO_STATE_ROOT を管理 scratch へ設定した。
Clippy の初回は volume Property 追加で大きくなった enum を指摘したため、Clip.volume と
schema-only ApiEnvelope の Request を Box にして解決した。JSON の形は変えない。

この環境準備・修正後の最終 run の成功を受け入れ証拠とする。以前の失敗 run を成功として数えない。
logs は管理 TMPDIR に保存し、既存 golden baseline は変更していない。


## host に残す gate と supervisor 所有の編集

**pending host run**: `CARGO_BUILD_JOBS=3 cargo test --workspace --locked`。
期待結果は全 suite が成功し、GPU / framebridge の tests を実機で検証すること。
今回の CPU / software MOV の成功を GPU / hardware codec / Metal golden の検証として扱わない。
既存 memory / length 上限を超える streaming、audio retime / effects / Generator（AUDIO-004）、
Media video/image drawing（COMP-002）、GUI clip Inspector は未実装。

Supervisor が docs/adr/README.md に ADR-0063 を追加し、docs/roadmap/milestones.md の
AUDIO-003 延期行を更新する。docs/README.md の検証文書リンク、backlog の判定 / render / check、
レビュー / commit は supervisor 所有。本 worker は指定の変更禁止ファイルを編集せず、
全体の受け入れ完了 / done を宣言しない。

## 変更ファイル一覧

開始時 worktree は clean。以下36 files が今回の deliverables。未追跡の新規 files も含む。

- `Cargo.lock`
- `apps/macos/Sources/KronelloCore/GeneratedAPI.swift`
- `crates/kronello-audio/Cargo.toml`
- `crates/kronello-audio/src/document.rs`
- `crates/kronello-audio/src/lib.rs`
- `crates/kronello-audio/tests/document.rs`
- `crates/kronello-cli/Cargo.toml`
- `crates/kronello-cli/src/main.rs`
- `crates/kronello-cli/tests/jobs.rs`
- `crates/kronello-cli/tests/machine.rs`
- `crates/kronello-media/src/audio.rs`
- `crates/kronello-media/src/export.rs`
- `crates/kronello-media/src/lib.rs`
- `crates/kronello-media/tests/audio.rs`
- `crates/kronello-model/src/builtin.rs`
- `crates/kronello-model/src/composition.rs`
- `crates/kronello-model/src/lib.rs`
- `crates/kronello-model/src/sequence.rs`
- `crates/kronello-model/tests/builtin_registry.rs`
- `crates/kronello-render/src/snapshot.rs`
- `crates/kronello-service/src/api.rs`
- `crates/kronello-service/src/edit.rs`
- `crates/kronello-service/src/jobs.rs`
- `crates/kronello-service/src/lib.rs`
- `crates/kronello-service/src/nle.rs`
- `crates/kronello-service/src/wire.rs`
- `crates/kronello-service/tests/api.rs`
- `crates/kronello-service/tests/nle.rs`
- `docs/adr/0063-document-audio-and-clip-volume.md`
- `docs/architecture/01-data-model.md`
- `docs/architecture/08-api-cli-mcp.md`
- `docs/architecture/14-jobs.md`
- `docs/architecture/audio-000.md`
- `docs/testing/audio-003.md`
- `schemas/api-v1.schema.json`
- `schemas/project-v1.schema.json`

## ホスト検証結果

supervisor が Apple Silicon（arm64）macOS 27.0、rustc 1.95.0、開始 revision `38f205058bcf7786c276de3d0e67b73d0869e969` 上の未コミット変更で実行した。

```sh
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py generate
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=5 cargo test --workspace --locked --no-fail-fast
python3 scripts/generate_swift_api.py --check
```

すべて exit 0。workspace test は GPU / framebridge を含めて 530 passed、0 failed、4 ignored。

## NLE-002 との統合

`m3-motion-authoring` への統合時に、video track の crossfade に音声を持つ Composition clip が含まれる場合を `UNSUPPORTED_FEATURE` とした（`crossfade_between_audible_composition_clips_fails_typed`）。audio track の Generator は NLE-002 の共有 Sequence 検証で `INVALID_CLIP` になるため、`retime_effect_generator_missing_assets_and_recursive_audio_fail_typed` の期待値を更新した。
