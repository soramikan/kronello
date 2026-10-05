# RENDER-003: 長尺・大解像度 movie export

検証日: 2026-10-06。基点 `c3173f3` と本変更を専用 worktree で検証した。
4つの受け入れ条件は下表のテスト / 実測で確認した。
backlog は統合側の必須 workspace gate 合格待ちのため `in_progress` とする。
設計は [ADR-0074](../adr/0074-bounded-streaming-movie-export.md)。

## 受け入れ条件との対応

| 条件 | 証拠 |
|---|---|
| 1. audio source / Bus と video payload の旧制限超過 | 実 WAV asset を601秒（28,848,000 stereo frames、旧28,800,000超）decode / drain / hash照合し、230,784,000 bytesのf32 spoolを4,096-frame窓からmix / PCM24 encode、601 framesのProResとmux。Generatorではない。4K 9 frames の実 render / tile / encode / mux は input RGBA 298,598,400 bytes（旧256 MiB超）を処理 |
| 2. 最終2面保持の削減とnode / halo予算 | `render_frame_tiles` のsinkに最大512×512面を渡し、movieはRGBA8 1面だけ保持。partial tile / shifted originの全画素比較。16段のsigma20 blurが512 MiBの保守的node allocation予算でbackend処理前に `UNSUPPORTED_FEATURE`、sink呼び出し0回 |
| 3. 画素・samples・時間の互換性 | `streamed_tiles_match_whole_frame_at_edges_and_stop_on_sink_failure` の全linear/display・metadata一致。既存shadow+blurのtiled/full比較にstream sinkを加え、scale0.5/1/2・x=512境界・partial tileの全linear画素一致。24 / 30000/1001 / 60000/1001、負開始・非ゼロ開始・非整数source trimのPCM24全samplesが従来whole mixとbit一致、video PTS / durationとaudio sample count一致。601秒movieも28,848,000 decoded samples全件bit一致 |
| 4. peak memory / I/O・失敗処理 | 下表のRSS・spool read/write・stage/file byte実測。frame checkpoint cancel、injected ENOSPC、encoder寸法拒否でoutput不在・一時directory空。途中の競合publication sentinelはno-clobber fenceでbyte不変。OS file-size制限による実source spool write / native FFmpeg write失敗でも旧artifact不変・temporaryなし |

## ホストと実測

Mac16,10、32 GiB RAM、arm64、macOS 27.0.1（26A434）、Rust1.95.0。
共有の固定FFmpeg9.0.2 LGPL build、software `prores_ks` / native `pcm_s24le`、
明示した `CpuReferenceBackend`、debug buildを使った。
同時に他の検証が動いており、elapsedは性能目標の合否判定ではない。

| 実測 | peak RSS bytes | elapsed seconds | 最終file bytes |
|---|---:|---:|---:|
| 601秒・1 fps・18×16、実601秒stereo WAV source、負開始-2秒 | 26,017,792 | 39.943 | 173,205,025 |
| 4K（3840×2160）・24 fps・9 frames | 109,985,792 | 74.094 | 913,096 |

601秒はblank Composition＋明示背景であり、同じ音声を全sample読み返して検査した。
4Kもblank Composition＋明示背景のCPU tile実行を含む。複雑な映像素材 / 多段effectsの4K速度を測ったものではない。
旧方式なら4Kの全linear/display面だけで265,420,800 bytesを保持し、
さらに全9-frame RGBA payload298,598,400 bytesを保持したため旧payload gateで失敗した要求である。
今回のRSSはmetadata、native encoder、allocator等を含むprocess peakとして`resource.getrusage(RUSAGE_CHILDREN)`から測った。

| 論理I/O / file実長 | 601秒 | 4K |
|---|---:|---:|
| audio spool成功write bytes | 230,784,000 | 0 |
| audio window成功read bytes | 230,784,000 | 0 |
| audio stage bytes | 173,089,419 | 108,739 |
| video stage bytes | 109,514 | 804,109 |
| published bytes | 173,205,025 | 913,096 |

`AvExportReport.streaming` の成功した読み書き量とfile metadataを記録した。
OSの`ru_inblock` / `ru_oublock`は双方0で、cache下の物理disk I/O throughputを確定していない。
論理bytesと物理disk bytesを同一視しない。
[測定JSON](render-003-measurements.json) はruntime / host、実測値、対象source SHA-256を含む。
生log / JSONはworktreeの`target/render3-*-measured.*`に保持する。

## 失敗の実行証拠

- `cancellation_and_encoder_failure_remove_all_temporary_outputs`: audio stage生成後、video frame1のcheckpointで停止し、outputもtemporaryも残さない。injected ENOSPC（OS code28）で `OUTPUT_IO_ERROR`、奇数寸法のencoder拒否でも同じ回収を確認。競合先がframe1で作った`existing` bytesは `OUTPUT_EXISTS`で維持する。
- `native_write_capacity_failure_removes_temporary_outputs`: childの`RLIMIT_FSIZE=65536`、SIGXFSZをignoreして実native PCM stage writeを失敗させる。`ENCODE_ERROR: write PCM packet: File too large (-27)`、新outputなし、既存artifact不変、一時directoryなし。
- `source_spool_capacity_failure_removes_temporary_outputs`: `RLIMIT_FSIZE=16384`で実source spool writeが `MEDIA_IO_ERROR` / OS code27。新outputなし、既存artifact不変、一時directoryなし。

容量試験は専用childへのOS制限であり、volume全体を物理的に満杯にした試験ではない。
ENOSPCは決定的なinjection、実I/O失敗はEFBIGで確認し、二つを区別する。
強制終了後の回収とresumeはRECOVERY-001。

## 再現

```sh
export PKG_CONFIG_PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib/pkgconfig
export KRONELLO_FFMPEG_LIB_DIR=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib
cargo test -p kronello-audio -p kronello-media -p kronello-render --locked
cargo test -p kronello-media --test streaming --locked --no-run
```

最後のcommandが表示した`streaming-*` executableを以下の`--binary`に指定する。
以下のsuffixはこの測定のdebug binary。feature組合せにより変わる。

```sh
python3 scripts/measure_export_streaming.py --binary target/debug/deps/streaming-e38184c2c5c97269 --test long_export_exceeds_source_and_bus_limits --output target/render3-long-measured.json
python3 scripts/measure_export_streaming.py --binary target/debug/deps/streaming-e38184c2c5c97269 --test four_k_export_exceeds_previous_video_payload_limit --output target/render3-4k-measured.json
python3 scripts/measure_export_streaming.py --binary target/debug/deps/streaming-e38184c2c5c97269 --test native_write_capacity_failure_removes_temporary_outputs --file-limit-bytes 65536 --output target/render3-capacity-measured.json
python3 scripts/measure_export_streaming.py --binary target/debug/deps/streaming-e38184c2c5c97269 --test source_spool_capacity_failure_removes_temporary_outputs --file-limit-bytes 16384 --output target/render3-spool-capacity-measured.json
```

## 必須検証と統合待ち

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`git diff --check`: exit0。
- `python3 scripts/backlog.py render` / `check`: exit0、77 tasks。
- `cargo test -p kronello-audio -p kronello-media -p kronello-render --locked` のhost実行: exit0、126 passed / 6 ignored。streamingのignored受け入れ4件は上記の手順で個別に実行し、すべてexit0。
- `cargo test -p kronello-service --test api --test nle_schema --locked`: exit0、12 passed。API schemaの追加optional fieldを再生成し、`generate_swift_api.py` / `--check`も一致。
- `cargo test --workspace --locked` のhost実行は、既存MCP `public_api_fixture_commands_return_schema_valid_success_from_real_binary` が20秒response timeoutでexit101（stdio:15 passed /1 failed）。この失敗を成功に読み替えない。isolated / `RUST_TEST_THREADS=1`の同testはexit0、1 passed、24.17秒。過去のaudio004 / vec005にも同じtimeout記録がある。
- API生成中に開始した前のworkspace runは埋込schemaの世代不一致で停止した。再生成後の独立schema suiteは全件pass。統合後にGUI側のschema追加と合わせて再生成し、完全workspace gateを再確認する。

Metal / hardware encoderの長尺性能、8K pixel上限解除、image sequenceの最終2面streaming、
report metadataの外部manifest化、物理disk throughput / 空volume試験は今回保証しない。
RENDERのsample・画素の意味は変えず、snapshot / request / codec意味版を変更しない。
