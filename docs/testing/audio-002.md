# AUDIO-002: 実時間音声再生と A/V 同期

設計: [ADR-0076](../adr/0076-buffered-device-clock-playback.md)。2026-10-05、branch `m3-audio2`、
base HEAD `376daa4c97a42a43827e60f47744efe57d3a40c2` の clean worktree からの未 commit 変更。
worker は共有 Cargo cache / target、managed TMPDIR、CARGO_BUILD_JOBS=3を使用する。
実 audio engine / Metal / SwiftPM / app / listening は本 sandbox の確認に含めない。
offline export の成功や native synthetic callback を実時間再生の受け入れ証拠に読み替えない。

## 条件と証拠

| 条件 | 自動確認 / 実装 | 実時間の受け入れ |
|---|---|---|
| 1. callback と作品更新 / disk / expression の分離 | AVAudioSourceNode は native SPSC consume のみ。prepare / producer queues と既存 project / Metal worker は別。`PlaybackNativeChecks` は production C consumer の allocation entry points を計測し、copy / silence / seek / underrun の全実行で0回。concurrent SPSC million-frame wraparound、release / acquire の whole-block publication を確認 | 実 OS thread / allocator / blocking call の Instruments review は pending host run |
| 2. 映像表示を audio clock へ合わせる | callback の sampleTime / hostTime、output latency 補償、checked integer frame floor。`testPresentationTickDoesNotQueryService` は frame 更新に scene / project / history 要求0件。native Metal submission host time をデバッグ応答へ追加、最新の frame へ coalesce | 実 engine + Metal の harness とmax offsetの報告は pending host run。physical scanout は別の loopback / capture |
| 3. seek / stop-resume / 24000/1001 / 30000/1001 | native clock test は arbitrary frames、latency、100000時間までの直接算術、floor bucket の境界。ring test はflush / absolute origin / resume整数、late-block破棄とunderrun。Swift tests はbinary resource、revision conflict、host / mute fallback、Sequence config | ≥30秒の実device samples、2回seek、stop/resume同一sampleを下記3 ratesで確認。pending host run |
| 固定 snapshot / export parity | Rust `prepared_blocks_match_export_evaluator_bits_and_remain_revision_pinned` はevaluator2のtone / retimeの全sample bits、逆順block、後編集と削除後の固定入力。`playback_bounds_errors_and_absent_audio_are_explicit` は4096-frame上限と出力の非変更。media `bounded_decode_rejects_before_exceeding_remaining_source_budget` は残budget、typed errorと旧decode bits | CPUの意味的証拠。実時間受け入れの代用にしない |
| FFI / UI | header signature testは12 Rust C ABI。Swift `testBinaryProducerAndHostFallback` / `testSequenceConfiguration`、app / harness / XCTest sourcesのdirect compiler、host harness executableのlink。mute KRButton / statusは既存tokens / owned focusを使用 | SwiftPM、Dark / Light、1440×900、keyboard focus、app listeningはpending host run |
| 測定結果の解析 | `scripts/test_audio_002_analysis.py` はsynthetic JSONLからNTSC offsetとfinish時のunderrunsを集計し、host-clock fallback / 30秒未満 / 不正grid / seek / resumeを拒否 | 解析器のunit evidenceのみ。synthetic traceをhost測定結果にしない |

`PlaybackNativeChecks.c` は malloc / calloc / realloc / free をproduction consumer translation unitで
instrumentし、consumer中の呼出しをassertする。AVAudioSourceNodeから呼ばれるSwift / OSの実allocator
activityまで測ったとはしない。ringはhardwareなしのsynthetic timestampsを使用する。
Swiftのhost fallback testsも実audio deviceの成功として数えない。

## worker の検証

以下はこのworktreeで実行するcommands。最終結果は末尾に記録する。

```sh
export CARGO_BUILD_JOBS=3
export KRONELLO_STATE_ROOT="$TMPDIR/audio002-state"
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py generate
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_
python3 scripts/build_ffi.py
python3 scripts/generate_swift_api.py --check
python3 scripts/test_audio_002_analysis.py
python3 scripts/check_gui_swift.py \
  --swiftc /Applications/Xcode-beta.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
  --sdk /Applications/Xcode-beta.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
  --disable-plugin-sandbox --run-checks
git diff --check
```

`--swiftc` / `--sdk`はsandboxのxcrun cacheがsystem temporary directoryへ書けないため明示する。
direct checkerはC11 native tests、Swift modules、app / gallery / AudioHarness、XCTest sourcesを
compile / typecheckし、host harnessをlinkする。AVAudioEngine / Metalのharnessは**起動しない**。
public schema / registryにentryを追加していないため、GeneratedAPI.swift / schemasは変更なし。
generated checkとworkspaceの既存schema一致testsでこの境界を確認する。

## 実 audio engine / Metal のホスト測定

このworktree root、macOS / Apple Siliconのaudio outputとMetalが使えるhostで実行する。
まず48 kHzの出力deviceを選び、device名 / nominal rate / buffer size / display Hz、OS / chip、
`git rev-parse HEAD`と未commit diffのrevision識別を測定結果と一緒に保存する。
harnessはaudioなし / muted / deviceなしのhost-clock fallbackを成功として受け入れない。
error / missing timestamp / 短すぎるdevice測定 / 不正なseek / resumeは非0終了。

```sh
CARGO_BUILD_JOBS=3 python3 scripts/build_ffi.py --release
swift build --package-path apps/macos -j 3
swift test --package-path apps/macos -j 3
python3 scripts/demo_audio_002.py --output-root target/audio002-host-001
```

demoは180秒のtone440とanimated Shapeを持つSequenceを24/1、24000/1001、30000/1001ごとに
作成し、`sequence.query`で既知文書を確認する。stdoutの3つの`swift run ... KronelloAudioHarness`
commandsを順に実行する。新しいoutput rootを使い、既存traceは上書きしない。

```sh
python3 - <<'PY'
import json, subprocess
from pathlib import Path
for row in json.loads(Path('target/audio002-host-001/manifest.json').read_text()):
    subprocess.run(['swift', 'run', '--package-path', 'apps/macos', '-j', '3', 'KronelloAudioHarness',
                    row['project'], row['sequence'], str(row['fps_num']), str(row['fps_den']),
                    row['revision'], row['trace']], check=True)
PY
python3 scripts/analyze_audio_002.py target/audio002-host-001/audio-*.jsonl \
  > target/audio002-host-001/report.json
```

各harnessは実AVAudioEngineをstartし、CAMetalLayerを既存Rust native previewへattachして40秒
測定する。7秒でframe137へseek、14秒でstopし1秒後に同じ整数sampleからresume、23秒でframe777へ
seek。frameごとのQuery列やCPU reference rendererを使わず、最新audio-clock frameだけを要求する。
windowを可視に保つ。underrun / producer / device errorsを隠さない。

JSONLのpresentation recordはrequested frame、device sample / callback sample / host timestamp、
native Metal submission timestamp、latency compensation、expected frame、offset、underruns /
missing frames、audio / video revision、clock epochを含む。event recordはseek / stop / resume / finish。
解析はepochごとの実callback sample進行を合計して≥30秒を要求する。seekのsampleは
`floor(frame*fps_den*48000/fps_num)`、stop / resumeは厳密一致を検査する。
期待する出力形式（値はhost実測で埋まる）:

```json
{
  "measurement": "real_audio_engine_metal_submission",
  "fps": "24000/1001",
  "observed_device_seconds": 0,
  "presentations": 0,
  "max_av_offset_frames": 0,
  "max_av_offset_ms": 0,
  "max_frame_grid_offset_ms": 0,
  "underruns": 0,
  "missing_frames": 0,
  "seek_count": 2,
  "stop_resume_sample_exact": true,
  "physical_scanout_or_loopback": "not_measured"
}
```

上の0は形式例であり測定値でも合格値でもない。実observed_device_secondsは30以上でなければ
解析が失敗する。max offset / underrunの合否値をworkerが創作しない。3率の報告と現場条件を
supervisorがレビューする。`queue.present`直後のtimestampはsubmissionのproxyで、displayの
scanout / loudspeakerの実到達を測った値とは呼ばない。必要なphysical A/V offsetは、同じhostで
loopbackまたは外部captureを併用して別の結果として記録する。
`max_av_offset_ms`はrational video frame startと補償済みaudio sample timeの差で、frame内のphaseも
含む。`max_av_offset_frames` / `max_frame_grid_offset_ms`はfloor frame gridの差だけを報告する。
正常なfloor presentationでもframe内phaseのoffsetは0とは限らない。

InstrumentsではAV audio render threadにdisk / JSON / Rust evaluator / mallocのcall stackがないこと、
producer / preparation / Rust project workerが別threadで動くことを確認する。
実appのDark / Lightでmuteと理由付きhost clock、seek / looping / stop-resumeを操作し、
button自身のfocusだけが点灯することを確認する。audio output切替で`AUDIO_DEVICE_CHANGED`と
停止、再playで新deviceへ準備する。次に44.1 kHz出力のAVAudioEngine conversion、負荷をかけた
producer underrun、編集後のblock切替 / audible latencyも測定し、48 kHz基準測定と分けて報告する。

## GUI-003 の統合と残件

Edit pageは`EditorModel.configurePlayback(target: .sequence(id), rateNum: ..., rateDen: ...)`、Motionは
nil/default Compositionを設定する。ページ側timerを加えず、既存`playing`bindingを使う。
Sequenceの選択UIは別workerの担当。EditorWindowのstatus / diagnostic hunkはmerge時に保持する。
GUI-003統合後にdirect checks / SwiftPM / 両ページの実playbackを再確認する。
backlog / ADR index / open questions / docs index / design-systemは変更せず、commitしていない。
AUDIO-002の実時間受け入れを本workerの完了報告だけでverifiedとしない。

## 実行結果（2026-10-05）

最終sourceは未commit。以下はworkerがこのworktreeで実際に実行した結果であり、
supervisorのhost測定結果はまだ受領していない。

| Command / 確認 | 結果 |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS（最終Rust source） |
| 指定CPU workspace command、fixtures生成後の先行run | PASS: 594 passed、1 ignored、10 filtered。最終のFFI test追加 / schema guard前のrunなので最終sourceの全suite合格に読み替えない |
| 指定CPU workspace command、最終Rust source / default concurrency | FAIL: 62 passed、1 failed。既存CLI `output_validation_failure_and_destination_race_preserve_deliverables` がRunning / 0 framesのまま60秒timeout |
| 同command、`RUST_TEST_THREADS=1` | FAIL: 154 passed、1 failed、2 filteredまで実行。既存MCP `public_api_fixture_commands_return_schema_valid_success_from_real_binary` がresponse 20秒timeout |
| CLI timeout testを`--exact`で単独再実行 | PASS: 1 passed、19 filtered |
| MCP timeout testを`--exact`で単独再実行、さらに`RUST_BACKTRACE=1 --nocapture` | 両run FAIL: 0 passed、1 failed、15 filtered。backtraceは`crates/kronello-mcp/tests/stdio.rs:666`の`template.migration_plan` callを示す。原因は未確定。test / timeout / CLI / MCP production codeを変更していない |
| 指定CPU workspace command + `RUST_TEST_THREADS=1` + `--skip public_api_fixture_commands_return_schema_valid_success_from_real_binary` | PASS: 594 passed、1 ignored、11 filtered（GPU名filterと追加MCP filterを含む）。最終Rust sourceのplayback / FFI / media testsを含む。指定commandそのものの合格ではない |
| `python3 scripts/build_ffi.py` | PASS: debug Rust cdylib / CLIをlocal link directoryへ配置。release buildではない |
| `python3 scripts/generate_swift_api.py --check` | PASS。schema / GeneratedAPI.swiftは変更なし |
| 上記direct Swift command（最新Swift source） | PASS: 28 PASS lines（focus source 1、native C 3、Swift checks 24）。新規Playback checks 3、app / XCTest sources typecheck、host harness linkを含む。SwiftPM / engine / Metal windowは起動していない |
| `python3 scripts/test_audio_002_analysis.py` | PASS: 3 synthetic unit tests |
| `python3 scripts/demo_audio_002.py --output-root "$TMPDIR/audio002-final-fixture-check"` | PASS: 3率のproject create / sequence.query / manifest。harnessは未実行 |
| `python3 -m py_compile`（追加3 scriptsとdirect checker） / `git diff --check` | PASS |

初回workspace runのgenerated media fixture不足は`fetch_fixtures.py` / `fixtures.py generate`で解消。
state-rootの書込制限はwritable `KRONELLO_STATE_ROOT`、xcrun cacheの制限は明示compiler / SDKで
解消して再実行した。`python3 -m unittest scripts/test_audio_002_analysis.py`はscript-local importで
失敗するため、表の実行可能script entry pointを使用する。

workspaceの最終指定commandは未合格であり、先行PASS / 単独PASSを最終全suiteのPASSに読み替えない。
SwiftPM build / test、実app、Instruments、manual listening、3率の実device / Metal測定、
physical loopback / scanout、GUI-003統合後の確認はpending host / integration run。


## M3 統合再検査（2026-10-06）

統合後の SwiftPM / FFI / 三入口比較の結果と未確認範囲は
[M3 統合受け入れ](m3-acceptance.md) に記録した。過去の worker 検査と現在の実機検査を区別する。

### 主エージェントの実デバイス / Metal 測定（2026-10-06）

主エージェントが統合 release FFI の `KronelloAudioHarness` を使い、実 AVAudioEngine と Metal submission を測定した。
24/1、24000/1001、30000/1001 fps の各runで38.58〜38.61秒のdevice clock進行、
930 / 929 / 1161回の映像提示、2回のseekと停止 / 再開を確認した。
stop / resume sample はすべて厳密一致、underrunとmissing sampleはすべて0。
最大の frame grid offset は1 / 1 / 2 frames（41.67 / 41.71 / 66.73ms）。
request時刻との差そのものの最大値は73.71 / 68.40 / 69.74msであり、frame格子へ量子化した値と区別する。

証拠: `target/m3-acceptance/audio-report.json` / `audio-host.log` と
`target/m3-completion-audio/audio-{24-1,24000-1001,30000-1001}.jsonl`。
CPU / GPUテストsuiteと並行した負荷条件の実測である。quiet条件での再測定を別記録とする。
physical scanout / loopback / 主観的listeningは今回の測定に含めず、機器の出力遅延全体の保証とは扱わない。
