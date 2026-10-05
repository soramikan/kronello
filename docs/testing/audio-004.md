# AUDIO-004: 版付きリタイム・effect・Generator・crossfade 音声の検証

設計: [ADR-0069](../adr/0069-versioned-stateless-audio.md)。2026-10-05、branch `m3-audio4`、
開始 HEAD `43a36927e2d0e740ff34da74a24a9791d245afd8` に対する未 commit の変更。
初回 job は clean worktree から実装し、中断後の再開 job は26ファイルの変更を引き継いだ。
「再開前」の結果は前 job の記録であり、再開 worker が新しく実行した結果は末尾へ分けて記す。
前 job の `audio004-workspace-verified.log` は再開 sandbox に存在せず、過去の raw log を
再確認できたとは扱わない。再開時の logs は新たに管理 scratch へ保存した。
本 worker の capability は CPU / native FFmpeg software のみ。GPU / hardware codec と full workspace
host run は未実行。以下の CPU 結果を host の検証へ読み替えない。

## 受け入れ条件と証拠

| 条件 | tests | status / 実際に確認する内容 |
|---|---|---|
| 1. TimeMap / 補間 / pitch / sync / source range | audio4 `resample_linear_pitch_source_range_and_fractional_trim_stretch`、`piecewise_resampling_breakpoint_and_trim_preserve_source_coordinates`、`resampling_shifts_pitch_and_requires_only_the_used_interpolation_tail`、`fractional_breakpoint_trim_uses_the_versioned_first_segment_extension` | CPU 確認済み。speed 1/2・2、PWL の breakpoint 前後、有理数 trim / stretch の source coordinate / source interval、fractional placement・fractional source position、two-tap interpolation tail、440→880 Hz resample。負 source / 不足 source を clamp / 無音にしない。fractional breakpoint の trim 先頭 bucket は ADR の新先頭 slope で外挿することを明示 assert |
| 2. pure / resource budget / fixed-input / arbitrary batches | audio4 `effects_curves_generators_are_owned_and_arbitrary_batch_order_is_exact`、`unsupported_contracts_nonfinite_overflow_and_resource_budgets_fail_typed`、`negative_effect_curve_unknown_version_and_curve_budget_are_typed_errors`、`inherited_bus_work_is_budgeted_even_for_disjoint_batches`、CLI jobs `audio4_retime_gain_generator_crossfade_fixed_job_matches_sync_ntsc` | CPU 確認済み。Curve Gain と tone / silence、逆順 batch と一括の全 sample bit 一致、compile 後の Curve / effects 編集の独立性、sample operations / map points / effect stack / Curve / Bus budget。実同期 export と実 worker の固定 job、後編集と Project 削除後の全 PCM / video 一致 |
| 3. audio crossfade | audio4 `audio_and_inherited_composition_crossfade_use_linear_half_open_sample_weights` と上記 CLI jobs | CPU 確認済み。Audio track Asset と audible Video CompositionClip、fractional floor range、outgoing / incoming 各側を独立に先頭・中点・最後・end の linear weights と比較、batch 分割、未知 transition version の拒否。NTSC の combined retime / Gain / Generator / crossfade を実 PCM24 export。probe.verify_av、zero-origin、4804 samples / 3 video frames |
| 4. 型付き失敗・無暗黙代替 | 上記 audio4 errors tests、`empty_sample_assets_and_extreme_crossfade_bounds_never_hide_errors_or_panic`、既存 document / mixing tests、CLI jobs の既存 missing / hash / clipping regression | CPU 確認済み。未知 map / effect / Generator / version、negative evaluated Gain、非有限 source / Gain、演算 overflow、source tail / budget、zero-sample asset placement の欠落と extreme crossfade 境界の整数 overflow を error にする。legacy profiles は新機能を拒否し、silence / speed 1 へ代替しない。旧 asset / hash / clipping の出版拒否も既存 tests で維持 |
| 旧契約 / schema / adapter | audio4 `legacy_versions_keep_existing_sample_bits`、既存 document `fractional_asset_trim_keeps_affine_sample_phase` / mixing / CLI AUDIO-003 tests、service API / nle_schema、Swift generator --check | evaluator 1 / profile 1/2 の旧 rounding / immutable plan / output を維持。Rust schemas と Swift を再生成し、Swift の旧 AudioRetimePolicy.reject と既存 effect variant 番号を維持。新 Generator は sequence.query で誤って unsupported と表示しない |

再開前に追加した audio4 は10 tests、再開時に budget regression を1 test追加し計11 tests。CLI jobs は1 test。document / mixing / AUDIO-003 の tests は既存であり、
今回の新規実装として数えない。document の旧 Generator rejection assertion は共有モデルが保存可能に
なったため INVALID_CLIP から evaluator 1 の UNSUPPORTED_FEATURE へ更新した。

## 版・API・error

- Audio evaluator 1 は compile / profile 1/2、evaluator 2 は compile_version / movie profile 3。
  `AvExportSnapshot::new` は schema 1、`with_audio` は2、`with_audio_profile(...,3)` は3。
  output mode の省略 explicit / 1、非空 explicit clips と document / silence の併用拒否を維持。
- `AudioRetimePolicy::ResampleV1`（wire resample_v1）、`EffectParameters::AudioGain {gain}`、
  `kronello.audio.gain` version 1、Generator silence / tone440 version 1、Crossfade version 1。
  audio versions は export profile に固定する。映像 RenderSnapshot SemanticVersions は変更しない。
- 既存 Timeline clip_place / clip_set_effects、render.export / render.submit を使う。capabilities は
  audio_resample_v1 / audio_gain_v1 / audio_generator_v1 / audio_crossfade_v1 と audio Gain effect を追加。
  shared project / API schema の外枠は1。Rust generator と GeneratedAPI.swift を再生成。
- new error `AUDIO_BUDGET_EXCEEDED`。既存 `UNSUPPORTED_FEATURE`、`INVALID_AUDIO_INPUT`、
  `AUDIO_SOURCE_TOO_SHORT`、`AUDIO_OVERFLOW`、`AUDIO_CLIPPING`、asset / hash / time codes を維持。
  旧 recursive compiler の budget は INVALID_AUDIO_INPUT のまま。shared Sequence validation の INVALID_CLIP / CLIP_OVERLAP / SOURCE_MISSING も維持。

## 再現環境

共有 CARGO_HOME / 注入された CARGO_TARGET_DIR と管理 scratch を使い、per-job Cargo cache は作らない。
今回の target は `70b9b9237417ff6ea8548cb66c877b1a1b19bf2d8aedf9bcc9fe31b58da49836`。

```sh
export CARGO_BUILD_JOBS=3
export TMPDIR=/Users/sora/.local/share/codex-bridge/scratch/worker_b431637d3032491d8de64f04556867fb
export CARGO_TARGET_DIR=/Users/sora/.local/share/codex-bridge/cache/targets/70b9b9237417ff6ea8548cb66c877b1a1b19bf2d8aedf9bcc9fe31b58da49836
export KRONELLO_STATE_ROOT="$TMPDIR/audio004-state"
export PKG_CONFIG_PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib/pkgconfig
export KRONELLO_FFMPEG_LIB_DIR=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib
export PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/bin:$PATH
```

外部 Noto fixture は既存 pinned OTF を読み取り専用の symlink で worktree の target/fixtures/external
へ参照し、`python3 scripts/fetch_fixtures.py --offline` で hash / size を確認した。network fetch はしていない。
`python3 scripts/fixtures.py generate` は9 media fixtures を生成・decode、exit 0。
これらは ignored target 配下であり deliverable の baseline を書き換えていない。

## 再開前の commands と結果

| command | 実行結果 |
|---|---|
| `cargo test -p kronello-audio --locked`（最終 focused） | exit 0、21 passed / 0 failed（audio4 10、document 5、mixing 6） |
| `cargo test -p kronello-cli --test jobs --locked -- audio4_retime_gain_generator_crossfade_fixed_job_matches_sync_ntsc` | exit 0、1 passed / 19 filtered（この focused 実行は全 jobs suite の成功を意味しない） |
| `KRONELLO_SCHEMA_UPDATE=1 cargo test -p kronello-service --test nle_schema --locked` | exit 0、1 passed、project / API schema を Rust から再生成 |
| `KRONELLO_UPDATE_API_SCHEMA=1 cargo test -p kronello-service --test api --locked` | exit 0、11 passed / 0 failed（capabilities 期待値を修正した最終再生成） |
| `python3 scripts/generate_swift_api.py` | exit 0、最終 schema shape 修正後に再生成 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0、最後の bounds / empty-source guards を含む検証。GPU / FrameBridge all-targets compile は hardware 実行ではない |
| `cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_` | 初回 exit 101、API capabilities の旧 effects 期待値。二回目 exit 101、既存 FIFO job timeout。最後の再実行は exit 0、529 passed / 0 failed / 1 ignored / 8 filtered、80 suite。下記の最終記録を参照 |
| `cargo fmt --all --check` / `python3 scripts/generate_swift_api.py --check` / `git diff --check` | exit 0（最終記録を参照） |

実装中の focused run では、旧 Generator validation code の assertion、PWL partial bucket の reference
calculation、追加 CLI test の audio dev-dependency、追加 test の borrow lifetime が失敗し、その都度修正した。
clippy は range pattern の style を一度拒否し、`1..=3` に修正した。これらを最終 pass と混同しない。
最初の zsh heredoc は managed temp 指定前に shell が temp を作るため拒否され、以後 bash と明示 TMPDIR
を使った。失敗したコマンドにファイル更新を実行したとは扱わない。

## 限界と supervisor-owned work

nested Composition / Media retime と node effects、retimed CompositionClip、audio effects on video clips、
Protected map / hold / loop、pitch preservation / anti-alias filter、任意 effect / Generator は未対応。
Audio track の unity Composition は Reject policy で継承する。Composition target の profile 3 は旧 unity audio。
Generator の sine は同じ platform / engine 内の random access を保証し、platform 間の sin bit 一致を保証しない。
実時間 playback と長尺 streaming は範囲外。

pending host run（上記 native / fixture / state 環境、実 GPU / hardware capability のある host）:

```sh
CARGO_BUILD_JOBS=3 cargo test --workspace --locked
```

期待結果は exit 0。既存 GPU / FrameBridge と hardware codec の条件を host が実際に確認し、revision /
platform / adapter / 結果を記録する。worker の CPU software ProRes 成功、filtered tests、all-targets compile は
この条件の代替ではない。host run の結果はまだ受領していない。

supervisor は ADR index の0069追加と0051 / 0062 / 0063の部分置換表示、backlog の status / render / check、
review / commit を担当する。本 worker は backlog / ADR index / open questions / docs README / design-system
を編集せず、commit / push / merge を行わない。受け入れの最終判定は supervisor に残す。

## 追加の検証経緯

二回目の CPU workspace は existing CLI jobs `fifo_one_slot_and_fixed_snapshot_survive_project_edits`
の second worker が Queued に残り60秒 timeout、exit 101。worker.log は startup / heartbeat started
までを記録し、原因は未確定。`cargo test -p kronello-cli --test jobs --locked -- --exact fifo_one_slot_and_fixed_snapshot_survive_project_edits`
の単独再実行は exit 0、1 passed / 19 filtered、1.85秒。単独成功を workspace 成功に読み替えない。
以後の全 workspace 再実行は他の Cargo command と並列にせず実行する。timeout や assertion を緩める
production / test 変更はしていない。

final review では generated Swift の schema description が旧 enum API を union に変え、effect variant
の挿入順が旧番号を変える点を発見した。ResampleV1 の variant comment を通常 comment にし、
AudioGain を enum の末尾へ追加した。再生成結果は旧 reject と GaussianBlur / DropShadow の variant
番号を維持する。Swift の build / native GUI 実行を確認したとは扱わない。

## 再開前の最終検証結果

最後のコード（empty-source / extreme-crossfade guards と audio4 の10 testsを含む）に対し、
他の Cargo command を並列実行せず、次の command をそのまま再実行した。

```sh
cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_
```

exit 0、**529 passed / 0 failed / 1 ignored / 8 filtered**、unit / integration / doctest の80 suite。
既存 FIFO test と新 AUDIO-004 の実 worker regression の両方が full jobs suite 内で成功した。
ignored は既存 store `snapshot_policy_evaluation`（明示実行する policy measurement）。
filtered は GPU tests であり、その成功を主張しない。raw log は管理 scratch の
`audio004-workspace-verified.log`。先の API assertion failure / FIFO timeout の記録も残す。

`cargo fmt --all --check`、`python3 scripts/generate_swift_api.py --check`、`git diff --check` は
この文書更新後にも exit 0。最終 clippy は `cargo clippy --workspace --all-targets --locked -- -D warnings`
で exit 0。API schema suite は11 passed、nle_schema は1 passed。focused audio は21 passed。
新しい dependency download はせず、Cargo.lock は CLI test 用の既存 workspace audio dependency 1行だけ。

## 変更ファイル

開始 clean の worktree で26ファイルを変更・追加した。新規ファイルは advanced.rs / audio4.rs /
ADR-0069 / 本記録の4件。

| prefix | files |
|---|---|
| `crates/kronello-audio/` | `src/advanced.rs`（新規）、`src/document.rs`、`src/lib.rs`、`tests/audio4.rs`（新規）、`tests/document.rs` |
| `crates/kronello-model/src/` | `effect.rs`、`sequence.rs` |
| `crates/kronello-media/src/` | `export.rs` |
| `crates/kronello-service/` | `src/api.rs`、`src/jobs.rs`、`src/nle.rs`、`tests/api.rs` |
| CLI / dependencies | `crates/kronello-cli/Cargo.toml`、`crates/kronello-cli/tests/jobs.rs`、`Cargo.lock` |
| generated | `schemas/project-v1.schema.json`、`schemas/api-v1.schema.json`、`apps/macos/Sources/KronelloCore/GeneratedAPI.swift` |
| ADR | `docs/adr/0069-versioned-stateless-audio.md`（新規）、`0051-nle-placement-and-retime.md`、`0062-video-generator-and-timeline-edits.md`、`0063-document-audio-and-clip-volume.md`（旧3件は状態・部分置換リンクのみ） |
| architecture / testing | `docs/architecture/audio-000.md`、`01-data-model.md`、`08-api-cli-mcp.md`、`docs/testing/audio-004.md`（新規） |

restricted backlog / ADR README / open questions / docs README / design system に差分はない。
`kronello-animation` / `kronello-time` / native codec / golden baseline / RenderSnapshot のコードは変更していない。
head は開始時のまま。コミットせず、host run と acceptance / status / integration は supervisor に残す。


## 再開時の review と検証

同じ HEAD / worktree の未 commit の26ファイルを review して引き継いだ。再開時に変更したのは
`src/advanced.rs`、`tests/audio4.rs`、`tests/document.rs`、ADR-0069、audio architecture、本記録の6ファイル。
既存の schema / Swift / API / CLI export regression は引継ぎの成果であり、再開時の新規実装とは扱わない。

review では、legacy 継承 mixer が placement と disjoint な request にも全長の Bus を確保・有限検査するのに、
sample-operation budget が overlap samples だけを計上していた点を修正した。evaluator 2 だけで
legacy entry ごとに `2 × request frames` を加算し、出力 allocation 前に拒否する。
`inherited_bus_work_is_budgeted_even_for_disjoint_batches` は4つの Composition clips と disjoint な
600秒 request を `AUDIO_BUDGET_EXCEEDED` で拒否することを確認する。evaluator 1 を変更していない。

read-only で確認した integration branch `m3-motion-authoring` の HEAD は
`87fd361118989fe2e0039efc58c1fb24c7af9c16`。GUI-001 の SceneNode は name / enabled を default 付きで追加し、
DocumentAudioPlan::walk は active_range 評価前に disabled node を continue する。
4つの audio fixture の SceneNode literal を legacy wire shape の deserialization へ変更し、新しい default
fields を追加した型にも対応する。current branch には GUI fields がまだなく、enabled の実行検証は
**pending integration run**。walk の既存 guard の hunk はこの変更で触れていない。supervisor は merge 時に
この guard を保持し、schema / Swift を結合後の型から再生成して disabled-node regression を再実行する。
worker は branch を merge していない。

| 再開 worker が実行した command | 結果 |
|---|---|
| `cargo test -p kronello-audio --locked` | exit 0、22 passed（audio4 11 / document 5 / mixing 6） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo fmt --all --check` | exit 0 |
| `python3 scripts/generate_swift_api.py --check` | exit 0 |
| `git diff --check` | exit 0 |
| `cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_` | 初回 exit 101、既存 MCP contract test の20秒 response timeout。audio22とCLI jobs20は成功。二回目 exit 101、既存 worker schema/hash test が Running のまま60秒 timeout。audio22と新 CLI regression は再度成功 |

logs は管理 scratch の `audio004-resume-audio.log` / `audio004-resume-clippy.log` /
`audio004-resume-workspace.log` / `audio004-resume-mcp.log` /
`audio004-resume-workspace-retry.log` / `audio004-resume-worker.log` /
`audio004-resume-workspace-threads3.log` / `audio004-resume-workspace-threads1.log`。host full workspace test は引き続き未実行。


再開時の初回 workspace は existing MCP `public_api_fixture_commands_return_schema_valid_success_from_real_binary`
が `tests/stdio.rs:164` の20秒 response timeout で失敗した。どの RPC が遅れたかは log から未確定。
timeout / assertion / production MCP を変更せず、isolated test と元の workspace command を再実行する。
この初回 run が後続 suites / doctests の成功を確認したとは扱わない。

統合後に supervisor が実行する commands（pending integration run、期待 exit 0）:

```sh
cargo test -p kronello-audio --test document --locked -- --exact disabled_media_node_is_silent
KRONELLO_UPDATE_API_SCHEMA=1 cargo test -p kronello-service --test api --locked
KRONELLO_SCHEMA_UPDATE=1 cargo test -p kronello-service --test nle_schema --locked
python3 scripts/generate_swift_api.py
python3 scripts/generate_swift_api.py --check
```

`disabled_media_node_is_silent` は integration branch の既存 test であり、未統合の current branch に
追加・実行していない。current branch にこの名前を指定して0 testsとなる run を検証と扱わない。
MEDIA-002 の movie / codec profile 追加との merge では、AUDIO-004 の movie profile 3 / evaluator 2
を他の意味へ再割当てしないことも supervisor が確認する。


isolated MCP command は exit 0、1 passed / 12 filtered、54.07秒:

```sh
cargo test -p kronello-mcp --test stdio --locked -- --exact public_api_fixture_commands_return_schema_valid_success_from_real_binary
```

この成功を workspace の成功に読み替えない。元の CPU workspace command を変更せず再実行したが、下記の別 test が timeout した。


再開時の二回目 workspace は existing CLI jobs `worker_checks_saved_schema_semantics_features_and_input_hash`
の blocker worker が Running / 0 frames、startup / heartbeat started の log のまま60秒 timeout、exit 101。
heartbeat_at_ms も初期値のままだった。原因を AUDIO-004 / CPU load / scheduler と断定できない。
新 audio4 export regression を含む他の19 jobs tests は成功した。worker test を単独で確認し、その後は
shared machine の test process 並列数を `RUST_TEST_THREADS=3` で制限して full CPU command を実行する。
この run でも assertions / timeout / filters / production code は変更していない。default concurrency の
workspace 成功とは区別して結果を記録する。


isolated CLI worker command は exit 0、1 passed / 19 filtered、2.40秒:

```sh
cargo test -p kronello-cli --test jobs --locked -- --exact worker_checks_saved_schema_semantics_features_and_input_hash
```

この成功も default workspace の成功を証明しない。次の full CPU run は exit 101、同じ既存 worker schema/hash test が今回は Queued のまま60秒 timeout。
他の19 jobs tests とaudio22は成功。threads=3 にしても stall は解消しなかった:

```sh
RUST_TEST_THREADS=3 cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_
```


最後に `RUST_TEST_THREADS=1` で全 CPU tests を逐次実行した。exit 0、**530 passed / 0 failed /
1 ignored / 8 filtered**、80 suites。ignored は既存 store `snapshot_policy_evaluation`、filtered は GPU tests。
新 AUDIO-004 export regression、既存 worker schema/hash test、MCP contract test、API11 / nle_schema1 を含む。
test を追加で filter / skip せず、assertions / timeout / production code を変更していない。
default concurrency の成功とは区別する。既存 worker stall / MCP timeout の root cause は未確定であり、
supervisor-owned follow-up として残す。

```sh
RUST_TEST_THREADS=1 cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_
```

この full run 後の final review で、crossfade unit test は incoming を mute して outgoing だけを
独立比較している点を補強した。incoming だけを audible にし、a の手前 / a / midpoint / b の手前 / b
を明示期待値と比較する assertions を追加した。production code は full run から変更していない。
補強後の `cargo test -p kronello-audio --locked` は exit 0、22 passed（audio4 11 / document 5 / mixing 6）。
`cargo clippy --workspace --all-targets --locked -- -D warnings` も exit 0。
final logs は `audio004-resume-audio-final.log` / `audio004-resume-clippy-final.log`。
full workspace は追加 assertions 前の test binary、最終 focused は追加後の binary を実行した。
production code / schema / API は両 run 間で同一。`cargo fmt --all --check`、
`python3 scripts/generate_swift_api.py --check`、`git diff --check` は最終文書更新後にも exit 0。

## 最終再開 worker の review と検証

2026-10-05、worker `worker_9c98f5df2fe14df099b95d96ee502c09` が同じ開始 HEAD の
未 commit の26ファイルを引き継いだ。実行環境は macOS 27.0 / arm64、rustc 1.95.0。
以前の worker の結果と以下の実測を区別する。今回の変更は `src/advanced.rs`、
`tests/audio4.rs`、ADR-0069、audio architecture、本記録の5ファイル。
他の21ファイルは引継ぎのまま保持した。schema / GeneratedAPI.swift も今回変更・再生成せず、
生成一致を検証した。前 worker の機能追加・budget 修正を今回の新規実装として数えない。

review で、evaluator 2 が Video CompositionClip の映像用 Property の存在だけで継承音声を
拒否していた点を修正した。clip-owned transform / opacity は AUDIO-003 と同じく映像だけに
適用し、音声へ転用しない。video clip の audio effects は引き続き型付き未対応。
`visual_clip_properties_preserve_inherited_audio_bits` は opacity=0 の Video CompositionClip で
evaluator 1 / 2 の全 Bus が映像用 Property の追加前と厳密一致することを確認する。
audio4 は計12 tests。今回追加した test はこの1件で、残る11件は引継ぎである。

共有 Cargo cache / target と管理 TMPDIR を使用した。compile / test は CARGO_BUILD_JOBS=3。
worker の state は `$TMPDIR/audio004-current-state`、FFmpeg prefix は前節と同じ LGPL runtime。
既存の fixture を利用し、font を `scripts/fetch_fixtures.py --offline` で hash / size 検証した。
fixture / golden baseline を変更していない。

| 今回実行した command | 結果 |
|---|---|
| `cargo test -p kronello-audio --locked` | exit 0、23 passed / 0 failed（audio4 12、document 5、mixing 6） |
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `python3 scripts/generate_swift_api.py --check` | exit 0、引継ぎの生成物が一致 |
| `python3 scripts/fetch_fixtures.py --offline` | exit 0、既存 pinned Noto fixture を確認 |
| `RUST_TEST_THREADS=1 cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_` | exit 0、531 passed / 0 failed / 1 ignored / 8 filtered、80 suites。今回の compatibility regression、CLI jobs20、API11、Rust schema 一致を含む |
| `git diff --check` | exit 0 |

workspace command は前 worker の既存 worker stall / MCP timeout 記録を踏まえ、初めから
RUST_TEST_THREADS=1 で実行した。default concurrency の成功や stall の原因究明を意味しない。
filter / assertion / timeout / worker production code は変更していない。
ignored は既存 store `snapshot_policy_evaluation`、filtered は指定の `gpu_` filter によるもの。
以前の失敗 run や単独 test の成功を今回の workspace 成功へ読み替えていない。
logs は今回の TMPDIR の `audio004-current-audio.log` / `audio004-current-clippy.log` /
`audio004-current-workspace.log`。この worker は以前の raw logs を再確認していない。

read-only で integration HEAD `87fd361118989fe2e0039efc58c1fb24c7af9c16` と
`disabled_media_node_is_silent` の存在を確認した。current branch の SceneNode はまだ name /
enabled を持たず、今回もその fields / walk guard を backport していない。
統合時の guard 保持・schema / Swift 再生成・disabled regression は前節の
**pending integration run** のまま。full workspace は **pending host run**:
`CARGO_BUILD_JOBS=3 cargo test --workspace --locked`。
GPU / hardware codec / host acceptance、default concurrency の再検証は未実行。
backlog / ADR index / open questions / docs README / design-system は変更せず、commit していない。
