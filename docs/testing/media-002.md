# MEDIA-002: 追加 movie profile / ALAC の検証

対象: branch `m3-media2`、開始 HEAD `880f62749fce7191466bda04223df35c23366fa0`、clean worktree からの未 commit 変更。
設計: [ADR-0068](../adr/0068-versioned-delivery-movie-profiles.md)。worker は CPU / software codec のみ。
VideoToolbox / GPU / AVFoundation は pending host run。受け入れの最終判定・backlog・commit は supervisor が担当する。

## 受け入れ条件と証拠

| # | 条件 | tests / 再現手順 | status |
|---|---|---|---|
| 1 | AV1 / H.264 / HEVC の版付き同期・job、固定 snapshot、PTS / duration、probe、no-clobber | media profiles `av1_alac_movie_roundtrip_pts_duration_and_publication`、CLI jobs `av1_delivery_sync_fixed_job_and_alac_match_quantized_evaluator`。同じ helper を hardware profile に適用する下記 host tests | AV1 の CPU 検証、hardware 2形式は pending host run。24 / 30000/1001 / 60000/1001 の非ゼロ絶対 range、3 frame の先頭・末尾・exclusive end、zero-origin / exact sample count / duration、両 hashes。実同期 / worker、投下後に volume / 映像を変更して Project を削除し全 decoded A/V を維持。提出後の既存 destination race で OUTPUT_EXISTS と既存 bytes保持 |
| 2 | VideoToolbox allow_sw=0、未対応 host は ENCODER_UNAVAILABLE、codec / transfer 記録 | profiles `delivery_profile_hash_versions_and_missing_hardware_are_closed`、既存 media selector / HARDWARE+HYBRID / native error tests、native km_encoder_open の既存 allow_sw=0、host tests / movie_profiles example | CPU は hardware 登録を除去した selector の型付き失敗・非選択を検証。実 device open / hardware encode は pending host run。AV1 success は software とだけ報告 |
| 3 | AAC / ALAC / AV1 音声の採用前契約、priming / padding / final samples / 配布・特許、採用形式の A/V sync / roundtrip | ADR-0068、profiles `alac_pcm24_bit_exact_partial_final_frames_and_no_clobber`、同期 / job helper、movie_profiles example | ALAC の CPU roundtripは全 channel sampleのbit一致。1 / 31 / 4095 / 4096 / 4097 / 4804 frames、±full scale、量子化誤差と最後の実 sampleを検証。AV1 MP4/ALAC の実sync。H.264/HEVC MOV/ALACはhost待ち。AAC-LC契約は文書化したが品質・配布/特許レビュー未実施で未採用。Opus Web配信も未採用 |
| 4 | 同梱 LGPLとsystem developmentの区別、閉API、未対応の型付き失敗、HDRはCOLOR-001 | service media `delivery_output_wire_versions_audio_and_container_contracts_are_strict`、API / nle_schema / Swift checks、example capabilities.verify_distribution | 閉形式のwire roundtrip、必須profile_version、AAC/未知版→UNSUPPORTED_FEATURE、拡張子→INVALID_MEDIA_INPUT、任意codec/args/shell・duplicate fields拒否。固定LGPL prefixを読取利用。build/manifest変更なし。package署名/再配置、system FFmpegのruntime検証、HDRは今回実行していない |

host で実行する4 tests は通常 suiteで明示 ignored にしており、ignored を hardware成功へ数えない。
新規操作は増やさず、registry の操作数は維持する。schemas/project-v1.schema.json は再生成コマンドを実行したが差分なし。
旧 ProRes / PCM24 native branch と movie profile 1/2/3・optional field省略時のhash/JSONは維持する。
旧 audio/media/CLI tests と AUDIO-004 legacy sample regressionをworkspaceで回帰実行した。

## API / error

- JobOutput: `av1_mp4 | h264_mov | hevc_mov`、profile_version必須=1、audioはexplicit/document/silence、省略explicit。
  audio_codecはalac/aac、省略alacだがaacはUNSUPPORTED_FEATURE。clips/backgroundは明示。
- 追加形式は evaluator 2 / AvExportSnapshot schema 3。MovieProfileの
  `av1_mp4_alac_v1 | h264_alac_v1 | hevc_alac_v1` は envelope hash / report.movie_profile に保存。
  report.audio_profile_version=3 は output.profile_version=1と別物。旧reportにはmovie_profileを追加しない。
- 新規media API: AvExportSnapshot.with_movie_profile / movie_profile、MediaProbe.verify_movie、MediaRuntime.encode_alac / mux_movie。
  公開要求には任意FFmpeg名/引数なし。内部codec名はreportだけに出る。
- 新規error codeはない。UNSUPPORTED_FEATURE、INVALID_MEDIA_INPUT、ENCODER_UNAVAILABLE、OUTPUT_EXISTS、
  ENCODE_ERRORと既存asset/audio/time errors。serviceのworker publicationは既存OUTPUT_VALIDATION_FAILED / lease errors。

## 再現環境・実行記録

Rust 1.95.0 / macOS arm64、共有CARGO_HOME、指定共有CARGO_TARGET_DIRと管理TMPDIR、CARGO_BUILD_JOBS=3。
固定LGPL FFmpeg 9.0.2 prefixを読み取り利用し、build / install / configureは実行しない。

```sh
export CARGO_BUILD_JOBS=3
export CARGO_TARGET_DIR=/Users/sora/.local/share/codex-bridge/cache/targets/9e8b3994cb8399da130551deaf796e35f240acff3b930430bc921e2172c16682
export TMPDIR=/Users/sora/.local/share/codex-bridge/scratch/worker_d5cc4c3729ad462c91861c98c39206ab
export KRONELLO_STATE_ROOT="$TMPDIR/media002-state"
export PKG_CONFIG_PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib/pkgconfig
export KRONELLO_FFMPEG_LIB_DIR=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib
export PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/bin:$PATH
```

最終コマンド結果は本書末尾に記す。実装中の失敗は次のとおりで、最終passと区別する。

- 初回profiles: 1 passed / 2 failed / 2 ignored。AV1 MOVは実muxで拒否、ALAC単独1 sample MOVはdecode不可。
  supervisor承認でAV1 MP4を採用し、ALACの内部stageをMP4に固定した。2回目はAV1成功 / ALAC1 sample失敗、
  stage変更後3 passed / 2 ignored。codec fallbackや許容差拡大でpassにしていない。
- 初回追加test compile: FrameRate / Rational にないhelperとRange<i64>.lenを参照し修正。
  CLI testに未宣言のkronello_timeを参照し失敗、既存serdeのtyped time decodeを使い修正。Cargo.lock変更なし。
- CLI focused run:64x32でSVT `Adaptive quantization can not be turned OFF when RC ON` により
  avcodec_open2がENCODE_ERROR。検証fixtureを64x64にして実エンコードを実行した。
  profileが全寸法を受理すると主張しない。次回は固定要求のbackgroundがJSON integer0からf32 0.0へ
  正規化された差をtestが誤比較したため失敗。typed JobOutputの正規化済み値と照合するよう修正した。

## pending host run（順序固定）

同じworktreeと上記Cargo/native環境を使う。新規出力rootを一つだけ作る。
各commandのexitとrevision / platform / encoder / execution / transfer_pathを保存する。
host_command_namesが未登録のsandboxのため、このworkerは以下を実行していない。

1. 同梱runtimeとAV1/ALACを確認する。

```sh
cargo run -p kronello-media --example capabilities --locked -- --verify-distribution
cargo test -p kronello-media --test profiles --locked
export KRONELLO_MEDIA_HOST_OUT="$TMPDIR/media002-host-$(date +%Y%m%d%H%M%S)"
mkdir "$KRONELLO_MEDIA_HOST_OUT"
cargo run -p kronello-media --example movie_profiles --locked -- "$KRONELLO_MEDIA_HOST_OUT/av1" av1
```

期待:exit0、全5libraryがLGPL/GPL・nonfree無効、SVT/ALAC登録、AV1software、ALAC bit一致。
exampleは3 NTSC frames / 4804 tone samplesのdelivery.mp4とreport.jsonを残す。

2. 実VideoToolboxのprofile roundtrip / timingを実行する。

```sh
cargo test -p kronello-media --test profiles --locked host_h264_alac_movie_roundtrip_pts_duration_and_publication -- --ignored --exact --nocapture
cargo test -p kronello-media --test profiles --locked host_hevc_alac_movie_roundtrip_pts_duration_and_publication -- --ignored --exact --nocapture
```

期待:それぞれ1 passed、hardware encoder名とCPU→hardware upload経路、全3 rateでPTS/sample/duration一致。
deviceが提供されないhostはENCODER_UNAVAILABLEで非ゼロ、software成功へ読み替えない。

3. hardware profileの同期/実worker固定snapshot・ALAC bit比較・destination raceを実行する。

```sh
cargo test -p kronello-cli --test jobs --locked host_h264_delivery_sync_fixed_job_and_alac_match_quantized_evaluator -- --ignored --exact --nocapture
cargo test -p kronello-cli --test jobs --locked host_hevc_delivery_sync_fixed_job_and_alac_match_quantized_evaluator -- --ignored --exact --nocapture
cargo run -p kronello-media --example movie_profiles --locked -- "$KRONELLO_MEDIA_HOST_OUT/h264" h264
cargo run -p kronello-media --example movie_profiles --locked -- "$KRONELLO_MEDIA_HOST_OUT/hevc" hevc
```

期待:各test1 passed、example exit0、H.264/HEVC hardware。3 frames / 4804 ALAC tone samplesのMOVを残す。

4. 保持した3filesをffprobeとAVFoundationで独立に確認する。

```sh
ffprobe -v error -show_format -show_streams -show_packets -of json "$KRONELLO_MEDIA_HOST_OUT/av1/delivery.mp4" > "$KRONELLO_MEDIA_HOST_OUT/av1/ffprobe.json"
ffprobe -v error -show_format -show_streams -show_packets -of json "$KRONELLO_MEDIA_HOST_OUT/h264/delivery.mov" > "$KRONELLO_MEDIA_HOST_OUT/h264/ffprobe.json"
ffprobe -v error -show_format -show_streams -show_packets -of json "$KRONELLO_MEDIA_HOST_OUT/hevc/delivery.mov" > "$KRONELLO_MEDIA_HOST_OUT/hevc/ffprobe.json"
cat > "$KRONELLO_MEDIA_HOST_OUT/inspect.swift" <<'SWIFT'
import AVFoundation
import CoreMedia
import Foundation
for path in CommandLine.arguments.dropFirst() {
    let asset = AVURLAsset(url: URL(fileURLWithPath: path))
    let playable = try await asset.load(.isPlayable)
    let duration = try await asset.load(.duration)
    let tracks = try await asset.load(.tracks)
    var codecs: [UInt32] = []
    for track in tracks {
        for description in try await track.load(.formatDescriptions) {
            codecs.append(CMFormatDescriptionGetMediaSubType(description))
        }
    }
    let report: [String: Any] = ["file": path, "isPlayable": playable,
        "duration_value": duration.value, "duration_timescale": duration.timescale,
        "codec_fourcc": codecs]
    print(String(data: try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys]), encoding: .utf8)!)
}
SWIFT
swift "$KRONELLO_MEDIA_HOST_OUT/inspect.swift" "$KRONELLO_MEDIA_HOST_OUT/av1/delivery.mp4" "$KRONELLO_MEDIA_HOST_OUT/h264/delivery.mov" "$KRONELLO_MEDIA_HOST_OUT/hevc/delivery.mov" > "$KRONELLO_MEDIA_HOST_OUT/avfoundation.jsonl"
```

ffprobe期待:codec AV1/H.264/HEVCとALAC 24-bit / stereo48k、start0、video3ticks、audio4096+708samples、
両snapshot metadata、packet PTS/duration保持。B-frameではDTSが負でもPTSはpresentation格子と照合する。
AVFoundationはisPlayable / duration / FourCCの実報告を記録する（command自体も未実行）。
AV1 / ALAC MP4のisPlayable=trueを既知事実として要求しない。false / load errorはplayer互換性の制限として記録する。
metadata照会を実再生/qualityの検証とは扱わない。

5. 統合host gate（GPU / FrameBridgeを含む。上記ignored hardware testsとは別）。

```sh
CARGO_BUILD_JOBS=3 cargo test --workspace --locked
python3 scripts/generate_swift_api.py --check
```

期待exit0。通常workspaceのignored hardware testsを「実行済み」としない。
package署名/再配置はRELEASE-001、AACレビュー/Opus採用、Windows/Linux runtime、HDR、長尺は別のfollow-up。


## CPU の保持 artifact / 独立 ffprobe

`cargo run -p kronello-media --example movie_profiles --locked -- "$TMPDIR/media002-av1-artifact" av1`
と `ffprobe -v error -show_format -show_streams -show_packets -of json "$TMPDIR/media002-av1-artifact/delivery.mp4"`
を今回のCPU sandboxで実行し、双方exit0。exampleは文書tone440 / evaluator2のPCM24量子化結果と
全4804 stereo samplesをbit比較し、全3 video framesのPTS/endも照合した。
artifactは管理scratchのmedia002-av1-artifact/delivery.mp4 / report.json、stdoutはmedia002-av1-report.json、
独立ffprobeはmedia002-av1-ffprobe.json、stderrはmedia002-av1-example.log。

実capabilitiesはFFmpeg9.0.2、指定LGPL prefix、全5libraries、distribution_eligible=true / development_only=false。
SVT-AV1 4.2.0 / libsvtav1 software、rgba→yuv420p / cpu_rgba_to_software_encoder、logical conversion
input49152 / output18432 bytes、CPU upload0。ALAC s32p / 24bit、48kHz stereo。
ffprobe video time_base1/30000、start_pts0、duration_ts3003、packet PTS0/1001/2002、各duration1001。
audio time_base1/48000、start_pts0、duration_ts4804、packet PTS0/4096、duration4096/708。
映像end3003/30000、音声last PTS4803/48000、exclusive end4804/48000、duration差は1/60000秒で1sample未満。
最終muxはdemuxed ALAC codecparのframe_sizeが0のためFFmpegの `track 1: codec frame size is not set`
warningを出すが、packet/sample/timingとbit比較は成功した。warningを消すためにpaddingやcodec代替はしない。
これらはCPU/native software測定で、VideoToolbox、AVFoundation、GPU、全package検証の証拠ではない。


## 最終 CPU checks

最初のworkspace CPU run（media002-workspace.log）はexit0、568 passed / 0 failed / 5 ignored / 10 filtered、
84 suite。途中で旧ProResの拡張子エラーをINVALID_REQUESTに維持するguardを戻したため、
その最終コードに対するworkspace / clippyを再実行し、最終結果を下記に記す。
追加4 ignoredはVideoToolbox profiles / CLI jobs、残る1 ignoredは既存storeのsnapshot policy測定。
filtered10は指定のgpu_ substring filterによる除外であり、成功として扱わない。
最終workspaceはRUST_TEST_THREADS=3で共有hostの同時テスト負荷を抑える。Cargo buildは常に3 jobs。

| 実行 command | 実結果 |
|---|---|
| `cargo fmt --all --check` | exit0（最終コード） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit0、media002-clippy-verified.log |
| `RUST_TEST_THREADS=3 cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_` | exit0、84 suites、568 passed / 0 failed / 5 ignored / 10 filtered、media002-workspace-verified.log |
| `cargo test -p kronello-media --test profiles --locked -- --nocapture` | exit0、3 passed / 2 ignored、media002-profiles.log |
| `cargo test -p kronello-service --test media --locked` | exit0、3 passed、media002-service-media.log |
| `KRONELLO_UPDATE_API_SCHEMA=1 cargo test -p kronello-service --test api --locked` | exit0、11 passed、API schema更新 |
| `KRONELLO_SCHEMA_UPDATE=1 cargo test -p kronello-service --test nle_schema --locked` | exit0、1 passed、Project schema差分なし |
| `python3 scripts/generate_swift_api.py` / `--check` | 両方exit0、GeneratedAPI.swift更新、最終check成功 |
| 上記 `movie_profiles ... av1` / 独立 `ffprobe` | 両方exit0、保持artifact / 測定値は上記 |
| `git diff --check` | exit0 |

最終workspace logで追加CPU tests 5件の `... ok` を個別確認した。
CLI focused log の失敗を上書きで隠さず、修正後の同testの成功は最終workspace logを証拠とする。
logsは上記管理TMPDIR内。別runtime / platform / hardware結果や長尺品質の証拠へ読み替えない。

## 変更ファイル

開始時のworktreeはcleanで、以下20 filesが今回の変更（新規4 filesを含む）。

- media: crates/kronello-media/src/audio.rs、src/export.rs、src/ffi.rs、native/media.c、
  tests/profiles.rs（新規）、examples/movie_profiles.rs（新規）。
- shared service: crates/kronello-service/src/api.rs、src/jobs.rs、src/lib.rs、src/wire.rs、tests/media.rs。
- 実CLI / worker検証: crates/kronello-cli/tests/jobs.rs。
- generated: schemas/api-v1.schema.json、apps/macos/Sources/KronelloCore/GeneratedAPI.swift。
- 日本語契約: docs/adr/0068-versioned-delivery-movie-profiles.md（新規）、
  docs/architecture/08-api-cli-mcp.md、12-platform-dependencies.md、14-jobs.md、audio-000.md、
  docs/testing/media-002.md（新規）。

schemas/project-v1.schema.json は再生成して差分なし。Cargo.lock、FFmpeg build script / manifest、
backlog、ADR index、open questions、docs README / design-system は変更なし。
コミット・push・merge、別worktreeへの書込み、hardware / GPU commandは実行していない。
