# AUDIO-011 VST3 / AU プラグインホスティングと信頼境界の受け入れ記録

状態: `m9-lane-b` 作業ツリーで確認済み（2026-10-08）。実装は
[ADR-0131](../adr/0131-audio-plugin-hosting-trust-boundary.md) の決定に従う。

## 実装範囲

### 信頼境界

サードパーティのプラグインコードは UI / service / render / evaluator /
job worker のいずれのプロセスにもロードしない。`dlopen` するのは detached
plugin helper（`kronello-plugin-host` binary、または `kronello` /
`kronello-mcp` の `plugin-helper` 再入パス）だけであり、`unsafe` は
`kronello-plugin` の `abi/` 配下のみに許可する。他の純粋層の
`unsafe_code = "forbid"` は変更していない。

```text
service / worker (kronello-plugin::run_helper)
  → 子プロセス spawn + JSON 1 往復（stdin → stdout, schema_version 1）
    → kronello-plugin-host / <program> plugin-helper
      → spec 再検証 + bundle_manifest_hash 再照合
      → abi::vst3（手書き COM 互換）または abi::au（AudioToolbox）で dlopen
      → describe / process → bounded JSON 応答
```

- `PluginSpec` は format・path・SHA-256 pin・component・version・
  parameter 表（最大 1,024 行）を持つ。`bundle_manifest_hash` は bundle 内の
  全 entry（sorted relative path + kind tag + file hash、symlink は link
  text を hash して辿らない）と解決済み module image の byte hash を pin
  する。検証は submit・worker 開始・helper の `dlopen` 直前の 3 箇所。
  bundle byte は project state・job record・protocol のいずれにも入らない。
- helper は要求の decode・spec 検証・pin 照合の後だけ plugin code に入り、
  自前 watchdog が deadline 超過で abort する。worker 側は stdout / stderr を
  別 thread で drain し、exchange 全体の timeout で強制 kill する。
- helper 解決順は `KRONELLO_PLUGIN_HELPER` → 同階層の
  `kronello-plugin-host` → 自 executable の `plugin-helper` 再入。
  `Service::with_plugin_helper` がテスト・埋込み用の注入点。

### VST3 ABI（Steinberg SDK 不使用）

`abi/vst3.rs` は公開 ABI の手書き宣言だけを使う:
`GetPluginFactory` / `ModuleEntry` / `ModuleExit`、COM 互換の
`IPluginFactory` / `IPluginFactory2` / `IComponent` / `IAudioProcessor` /
`IEditController` / `IConnectionPoint` を、ProcessContext なしの
`ProcessData`（`numSamples` + 32-bit float planar buffers）で処理する。
TUID は 16 byte の hex class id。対応範囲は stereo（または channel
一致の single bus）audio effect で、非対応 bus 構成・複数 module・
エクスポート欠落・QI 失敗は `UNSUPPORTED_FEATURE` / `PLUGIN_FAILED`。

### AudioUnit（macOS）

`abi/au.rs` は AudioToolbox / CoreFoundation framework を helper 内で直接
link し、`AudioComponentFindNext` / `AudioComponentInstanceNew` /
`AudioUnitSetParameter` / `AudioUnitRender` を使う。component は
`type:subtype:manufacturer` の four-character code 三組、version pin は
`AudioComponentGetVersion` の hex。file-backed component は bundle pin、
path のない built-in component は triplet + version で pin する。
macOS 以外では `UNSUPPORTED_FEATURE`。

### 型付きエラー

`PluginError` は `INVALID_REQUEST`（spec・deadline・io 形状）、
`PLUGIN_MISSING`、`ASSET_HASH_MISMATCH`、`PLUGIN_PROTOCOL`、
`PLUGIN_FAILED`、`PLUGIN_TIMEOUT`、`UNSUPPORTED_FEATURE`、`IO_ERROR` を返す。
helper 内の panic / abort / signal は abnormal exit として `PLUGIN_FAILED`。
worker timeout は `PLUGIN_TIMEOUT`、helper watchdog abort は `PLUGIN_FAILED`
として区別される。不正・空・余分な helper 出力は `PLUGIN_PROTOCOL`。

### service / model / job 統合

- `audio.plugin_probe` は bounded describe 交換で `PluginReport`
  （name / vendor / version / classes / parameters / buses）を返す。
- `audio.plugin_process` は `plugin` payload の固定入力ジョブを記録する。
  `PluginJobInput` は project path・document hash（record の
  `snapshot_hash`）・locked Asset・stream_index・pin 済み spec・
  destination・deadline を持ち、bundle byte と render state を含まない。
  worker は locked stream を decode → f32 stage file → helper →
  48 kHz stereo PCM24 `.mov` を既存の receipt / no-clobber publication で
  確定する。kind 照合は `output_profile["plugin"]` の存在。
- model は `kronello.audio.plugin` effect（reserved 49xx descriptor、
  `bundle` / `format` / `component` / `sha256` / `plugin_version` /
  `parameters` の `param` / `value` table）を受理・検証するが、解決は
  `UNSUPPORTED_MODEL_FEATURE`。`kronello-audio` evaluator も同じく
  `UNSUPPORTED_FEATURE` で拒否し、実行入口は `audio.plugin_process` に
  限定する。NLE split は plugin binding property を clip へ remap する。
- capabilities: `features` に `audio_plugin_host_v1`、`effects` に
  `kronello.audio.plugin`。`schemas/api-v1.schema.json` /
  `schemas/project-v1.schema.json` /
  `apps/macos/Sources/KronelloCore/GeneratedAPI.swift` を再生成済み。

### deterministic fixture

`test_support::build_fixture_bundle` がテスト実行と同じ `rustc` で単一
stereo gain plugin（class id `6b726f6e656c6c6f746573746761696e`、
normalized param id 0）を `.vst3` bundle layout で compile する。
`KRONELLO_PLUGIN_FIXTURE_CRASH` / `KRONELLO_PLUGIN_FIXTURE_HANG_MS` /
`KRONELLO_PLUGIN_FIXTURE_LOG` が crash・hang・lifecycle marker の注入点。

## 受け入れ証拠

worktree ルートで実行（2026-10-08、macOS arm64、Rust 1.95.0、debug build）。

```sh
cargo test -p kronello-plugin --features test-support --locked   # 13 件成功
cargo test -p kronello-service --locked --test plugin            # 4 件成功
cargo test -p kronello-service --locked --test api               # 12 件成功
cargo test -p kronello-cli --locked --test plugin                # 3 件成功
cargo test -p kronello-model --locked --test effects             # 6 件成功
cargo test -p kronello-audio --locked --test dsp_effects         # 4 件成功
cargo fmt --all --check                                          # OK
cargo clippy --workspace --all-targets --locked -- -D warnings   # OK
KRONELLO_STATE_ROOT=<isolated> cargo test --workspace --locked   # 全て成功（後述の環境注記）
python3 scripts/backlog.py check                                 # OK
```

schema / Swift 生成の一致:

```sh
KRONELLO_SCHEMA_UPDATE=1 cargo test -p kronello-service --locked --test nle_schema   # 一致
python3 scripts/generate_swift_api.py                                              # drift なし
```

| 確認項目 | テスト / 内容 |
|---|---|
| VST3 fixture describe | `host.rs::fixture_describe_reports_class_and_identity`: helper 経由で name / vendor / version / class id が返る |
| VST3 process + unload | `fixture_process_roundtrip_and_unload`: param 0 = 0.5 の gain が全 sample に適用され、`module_entry` → `process` → `module_exit` の順で lifecycle log に記録 |
| crash | `fixture_crash_is_typed_plugin_failed`: fixture の abort が `PLUGIN_FAILED` |
| timeout | `fixture_hang_killed_by_worker_timeout`（worker kill → `PLUGIN_TIMEOUT`）と `fixture_hang_killed_by_helper_watchdog`（helper abort → `PLUGIN_FAILED`）の両経路 |
| missing bundle | `missing_bundle_is_typed` → `PLUGIN_MISSING` |
| hash mismatch | `hash_mismatch_is_typed` → `ASSET_HASH_MISMATCH`（helper spawn 前） |
| unknown class / unsupported ABI | `unknown_class_is_typed` / `non_module_file_is_unsupported_abi` |
| malformed protocol | `malformed_helper_output_is_protocol_error` / `missing_helper_binary_is_typed` |
| spec determinism | `spec_validation_and_manifest_determinism`: manifest hash の決定性・parameter / sha256 検証 |
| Audio Unit | `audio_unit_delay_describe_and_process`（macOS 実機で system delay unit の describe + render）、`audio_unit_is_typed_unsupported_off_macos` |
| model binding | `kronello-model tests/effects.rs`: 49xx descriptor の検証、parameter table 形状、`UNSUPPORTED_MODEL_FEATURE`、split remap |
| evaluator 拒否 | `kronello-audio tests/dsp_effects.rs::audio_plugin_effect_is_rejected_by_the_evaluator` |
| service probe | `tests/plugin.rs::probe_reports_fixture_identity_through_detached_helper`（実 helper 経由）、`probe_failures_are_typed_before_and_after_spawn`（`INVALID_REQUEST` / `PLUGIN_MISSING` / `PLUGIN_PROTOCOL`） |
| service submit | `process_submit_validates_request_project_and_asset` / `process_submit_rejects_non_audio_streams_and_kinds`（project・asset・stream・destination・spec の型付き検証） |
| API sweep | `tests/api.rs`: `audio.plugin_probe` を実 helper + fixture で実行し response schema に適合、`audio.plugin_process` の submit が `JobRecord` を返す |
| CLI e2e | `tests/plugin.rs::plugin_probe_and_process_run_end_to_end`: `audio plugin_probe` / `audio plugin_process` が detached worker 経由で PCM24 `.mov` を公開し report に snapshot / plugin identity を記録。`plugin_failures_are_typed_through_the_job`、`plugin_helper_reentry_speaks_the_protocol`（`<program> plugin-helper` 再入） |
| job 固定入力 | `FixedInput::plugin` は render state を持たず、`output_profile["plugin"]` と document hash / destination の厳密一致で照合。receipt 後の published `.mov` byte hash が receipt と一致しない場合は publish を拒否 |

### 環境注記（本タスク起因ではない既存問題）

このホストの実ユーザー state root
（`~/Library/Application Support/Kronello/jobs.sqlite3`）は別 worktree
（`kronello-m9-lane-f`、job DB schema `user_version=2`）の実行で
version 2 に更新済みであり、本ブランチの `kronello-jobs` は `version > 1` を
`UNSUPPORTED_SCHEMA_VERSION` で拒否する。そのため ambient state root を使う
`kronello-ffi tests/boundary.rs::subscription_detects_external_revision_and_job_snapshot`
は本変更の有無に関係なく失敗する。`KRONELLO_STATE_ROOT` を一時 directory に
隔離した workspace 実行では全 test が成功し、当該テストも単独で成功することを
確認済み。実ユーザー DB の schema を本タスクでは変更しない。

## 残件

- VST3 の対応範囲は stereo audio effect の `process` + normalized parameter
  のみ。MIDI / note expression・multi-bus・`IProcessContextRequirements`・
  preset / state blob（`IComponent::getState`）の取り込み、editor GUI は
  未対応（`UNSUPPORTED_FEATURE`）。広いプラグイン互換性の検証は別タスク。
- Audio Unit は macOS のみ。Linux / Windows の `audio_unit` spec は
  `UNSUPPORTED_FEATURE` で、他フォーマットの追加も未着手。
- 実 third-party plugin（商用 VST3 / AU）での受け入れは in-repo fixture のみで
  行い、ライセンス上 SDK / 実プラグインをリポジトリに持ち込んでいない。
- helper は process 分離のみ。sandbox profile（Seatbelt / seccomp 等）の
  強制は未適用であり、悪意のある plugin に対する OS レベル confinement は
  後続の強化対象（ADR-0131 の範囲は crash / hang / 型付き失敗の隔離）。
