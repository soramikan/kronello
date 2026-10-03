# AUDIO-000 の検証

契約: [ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)、API / 依存境界: [基本音声](../architecture/audio-000.md)。以下は library API の CPU 参照・native codec 検証。新しい service / CLI / MCP command と GPU 常駐経路の検証を表さない。

## 受け入れ条件とテスト

| 受け入れ条件 | 再現可能な検証 |
|---|---|
| 素材音声を48kHzへ変換しクリップ音量を適用してBusへミックスする | media `bundled_pcm_fixture_decodes_exact_stereo_samples`: 同梱 48 kHz stereo WAV の全 4,800 frames を PCM16 の解析値と厳密比較。`swresample_converts_mono_stereo_rates_and_drains_tail`: 44.1 / 32 kHz mono、96 kHz stereo を 48 kHz へ変換、4,800 frames、左右の対応、sine の内部 sample 値、drain 後の末尾を確認。audio `overlap_gain_trim_negative_placement_and_silence_preserve_float_headroom` / `source_trim_and_placement_boundaries_use_absolute_floor`: gain、重なり、trim、負配置、無音、1 超の Bus を比較 |
| 音声サンプル位置を絶対時刻から計算し、映像との同期をサンプル精度で検証する | audio `absolute_sample_boundaries_cover_fractional_frames_without_drift`: 24 / 30000/1001 / 60000/1001 fps の既知 batch 長、負 frame、1,000,000 frame、24 時間の index を i128 の解析式と比較、誤差 `[0,1 sample)`、隣接境界一致。`partitioned_mixing_matches_whole_bus_at_ntsc_boundaries_and_arbitrary_request_order`: 逆・任意順の batch を結合して全体の Bus と厳密比較。media `export_fixed_snapshot_muxes_av_with_exact_pts_and_sample_precision_at_three_rates`: 非ゼロかつ非整数 sample の開始・終了境界を含む export の映像 PTS、音声 sample count、duration 差 `<1/48000` 秒、開始 PTS 0 を確認 |
| 書き出しで映像と音声を同じ固定snapshotからmuxする | 上記 media export テスト: owned RenderSnapshot + 音声配置を serialize / restore、投稿後の元 Project.name / assets を編集してから既存 CPU `render_frame` を使って ProRes + PCM24 MOV を export。frame metadata と音声 report の RenderSnapshot hash、音声配置を含む export hash、MOV の両 hash を照合。mux 後の映像を decode_at、音声を decode して全 sample を純粋 mixer と PCM24 の 1 LSB 以内で比較。gain だけ変えると envelope hash だけ変わることも確認 |

音声の pure tests は `crates/kronello-audio/tests/mixing.rs`、native / export tests は `crates/kronello-media/tests/audio.rs`。

## 量子化と失敗経路

- audio `pcm24_rounding_full_scale_endpoints_and_no_implicit_clipping`: signed PCM24 の ±1、±0.5、half-LSB の ties away from zero。Bus の過大振幅は Reject で失敗、明示 Saturate は clipping count を返す。
- audio `invalid_gain_nonfinite_sources_overflow_missing_and_short_assets_fail`: 負 / 非有限 gain、非有限 source、積 overflow、時刻 overflow、source 欠落・不足、Bus budget。
- media `unsupported_channel_layout_and_stream_selection_fail`: 3 channel を拒否、存在しない / overflow stream index を拒否。speaker mask のない basic WAV の mono / stereo は上の rate / fixture tests で明示規約を検証する。
- media `asset_audio_hash_mismatch_and_missing_fail_without_substitution`: Asset hash の毎回確認、hash 不一致、欠落。
- media `pcm24_roundtrip_quantization_clipping_policy_and_output_rollback`: native PCM24 を再 decode、sample count / start / duration、全 channel sample の量子化誤差、clipping policy / count、既存 file の保持、失敗時に出力がないこと。
- media `export_rejects_bad_ranges_versions_missing_assets_clipping_and_existing_outputs`: 非 frame 境界、未知 envelope schema、snapshot にない AssetId、過大 gain、既存出力を拒否。
- media `mux_rejects_duration_mismatch_before_publication`: 長さが違う stage streams を mux して公開しない。
- 既存 media 3 unit / 4 asset / 7 video tests も回帰実行する。capabilities の library count は 4 から 5 へ更新した。

実装した非連続 source PTS、途中の format 変更、HDR / gamut 外の拒否については、このテスト一覧で個別 fixture を作った検証は行っていない。これらの未測定範囲を、正常素材のサンプル精度・codec roundtrip の成功へ含めない。

## 再現コマンド

Rust 1.95.0、FFmpeg development headers / pkg-config、fixture 生成用の FFmpeg / ffprobe を使う。新しい外部 codec 依存はない。

```sh
cd /Users/sora/Repositories/soramikan/kronello/.worktrees/audio
python3 scripts/fixtures.py generate
cargo fmt --all --check
cargo clippy -p kronello-audio -p kronello-media --all-targets --locked -- -D warnings
cargo test -p kronello-audio --locked
cargo test -p kronello-media --locked
python3 scripts/backlog.py check
git diff --check
```

同梱 LGPL prefix を用いる追加検証:

```sh
export KRONELLO_FFMPEG_LIB_DIR=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib
cargo run -p kronello-media --example capabilities --locked -- --verify-distribution
cargo test -p kronello-media --locked
```

これは現在の FFmpeg 9 headers と同じ ABI の prefix を指定する手順。別 major を指定して暗黙に system library へ戻さない。fresh な同梱 headers build を行う場合は supervisor の host 側で `PKG_CONFIG_PATH=$PWD/target/native/ffmpeg-lgpl/lib/pkgconfig` と別 `CARGO_TARGET_DIR` を明示する。

## 今回の実行結果

2026-10-04、Codex が branch `m2-audio-000`、base HEAD `6b232199edf2419728a90ef45de8e2701cc60957` + 本変更、macOS 27.0 / arm64 の指定 worktree 内で実行した。これはこの job の実行結果であり、MEDIA-001 文書の過去の supervisor 提供測定を転記したものではない。

- `cargo check -p kronello-audio -p kronello-media --all-targets`: 成功。新しい workspace member を Cargo.lock に反映した。
- `python3 scripts/fixtures.py generate`: 成功、9 media fixtures を生成・decode。
- `cargo test -p kronello-audio --locked`: 6 passed、0 failed / ignored、unit / doc-test は各 0。
- `cargo test -p kronello-media --locked`: 22 passed（unit 3 / assets 4 / audio 8 / media 7）、0 failed / ignored、doc-test 0。開発用 Homebrew FFmpeg 9.0.2（pkg-config libavcodec 63.1.102、library directory `/opt/homebrew/Cellar/ffmpeg/9.0.2/lib`）。この開発用構成を配布入力として採用していない。
- 上記 LGPL override の `cargo test -p kronello-media --locked`: 同じ 22 passed、0 failed / ignored。`target/audio-000/media-lgpl.log` に記録。
- LGPL override の `capabilities --verify-distribution`: exit 0。FFmpeg 9.0.2、`substituted=true`、`distribution_eligible=true`、`development_only=false`、avutil / avcodec / avformat / swscale / swresample の全 5 library が LGPL version 2.1 or later、GPL / nonfree 無効、ProRes / native PCM24 / SVT-AV1 encoder を確認。`target/audio-000/capabilities-lgpl.json` に記録。既存 prefix を読取利用し、同梱ビルドをこの job で再作成してはいない。

初回の audio tests は basic WAV の unspecified layout を拒否して失敗したため、1 / 2 channel の意味を ADR に明示して修正した。次の export tests は NTSC の最終 video frame duration が native encoder へ明示されず、MOV が 2 frame 分の duration を返して失敗した。各 input frame の長さを 1 time_base tick として frame / packet に渡し、全 3 rate で厳密に検証した。別形式への fallback や受け入れ誤差の拡大は行っていない。

最終 `cargo fmt --all --check` と `cargo clippy -p kronello-audio -p kronello-media --all-targets --locked -- -D warnings` は exit 0。三つの library 受け入れ条件を確認して AUDIO-000 を `done` に更新し、`python3 scripts/backlog.py render` で BACKLOG.md を再生成した（exit 0）。`python3 scripts/backlog.py check` は exit 0（60 tasks）、`git diff --check` も exit 0。

## host 側の残り範囲

supervisor の指示により workspace Cargo はこの job で実行していない。統合した checkout で以下を実行する:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/backlog.py check
git diff --check
```

GPU backend の注入は API として可能だが、この job は CPU reference を明示選択した。GPU backend の実機 A/V export、配布 package 全体の再配置・署名、Linux / Windows、長尺 streaming、実時間再生は未検証。AUDIO-000 の三つの library 受け入れ条件は CPU / native / 既存 LGPL runtime の検証で確認し、これらの別 gate と区別する。
