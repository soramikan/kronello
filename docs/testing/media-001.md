# MEDIA-001 の検証

対象: FFmpeg 素材 I/O、rational PTS、素材参照、LGPL native build。設計は [ADR-0048](../adr/0048-media-native-build-and-asset-verification.md)。GPU 常駐 decode / working-space color 変換の受け入れとは区別する。

## 依存と build

Rust 1.95.0、C compiler、pkg-config、FFmpeg development headers が必要。Rust は libav 本体を静的リンクせず、独自 C shim と共有 library の runtime loading を使う。FFmpeg source の構造体を Rust へ bindgen しない。通常の macOS development build は Homebrew FFmpeg、Linux CI は Ubuntu の `libavcodec-dev libavformat-dev libavutil-dev libswscale-dev` を使用する。これらが GPL 構成でも development_only と報告し、配布物には使わない。

同梱 build は Python **3.12 以上**、CMake、Meson、Ninja、make、C/C++ compiler が必要。SVT の `EXCLUDE_HASH=ON` は GNU ld の `--build-id=none` を使うため Linux のみ有効にする。macOS では指定しない。source は manifest の HTTPS URL と SHA-256 で固定する。GPL / nonfree / network / autodetect を無効にし、shared FFmpeg + shared SVT-AV1 / dav1d を作る。完了時に全 library の loaded license / configuration / ABI と AV1 / ProRes を検証し、license 原文と PATENTS、source manifest、共有ライブラリ hash の receipt を prefix へ置く。

```sh
python3 scripts/build_ffmpeg_lgpl.py --jobs 8
python3 scripts/build_ffmpeg_lgpl.py --verify-only
export PKG_CONFIG_PATH="$PWD/target/native/ffmpeg-lgpl/lib/pkgconfig"
export KRONELLO_FFMPEG_LIB_DIR="$PWD/target/native/ffmpeg-lgpl/lib"
export PATH="$PWD/target/native/ffmpeg-lgpl/bin:$PATH"
export CARGO_TARGET_DIR="$PWD/target/media-lgpl"
python3 scripts/fixtures.py generate
python3 scripts/fixtures.py check --generated target/fixtures/generated
cargo run -p kronello-media --example capabilities --locked -- --verify-distribution
cargo test -p kronello-media -p kronello-service --test media --locked
cargo test -p kronello-media --test assets --locked
cargo run -p kronello-media --example hardware_roundtrip --locked -- target/media-lgpl/hardware-roundtrip
```

`--offline` は既存 download cache を hash 検証して使う。prefix は新規 directory のみ受け付け、既存 prefix を再利用する場合は `--verify-only`。build / verify は欠落や hash / license 不一致で非ゼロ終了する。release artifact の再配置・署名・全配布パッケージの依存走査は release packaging の範囲であり、本 script の prefix 生成と混同しない。

runtime override は `KRONELLO_FFMPEG_LIB_DIR`（lib directory そのもの）。指定先の canonical path と構成を capabilities に記録する。headers と別 ABI major、欠落 directory / symbol では `FFMPEG_UNAVAILABLE`。release verification は FFmpeg 9.x + LGPL と必須 software codec を要求する。

## 受け入れ条件と証拠の対応

条件の正本は `docs/backlog/backlog.json` の MEDIA-001。以下は全7条件をその順序・文言で対応付けた完了記録。証拠ファイルは特記のない限り `target/media-001-host/` 配下であり、ホスト実行者・環境とコマンドは末尾の「ホスト検証と完了記録」に記載する。

| # | 受け入れ条件 | 確認内容と証拠 |
|---|---|---|
| 1 | VFR/B-frame/seek後の対象PTSを確認する | `media-tests2.log` の `cfr_vfr_bframes_seek_exact_intervals_and_drain` が成功。CFR 5 rate、VFR、native MPEG-4 B-frame の7 filesについて seek 順4,0,3,1,5,2,0,5、各 frame の start / midpoint / end 直前、stream 外・最終 end を厳密比較。`fixtures.log` は9 media の生成・decode と16 entries / 9 scenes の検証成功を記録。B-frame fixture は LGPL encoder `mpeg4 -bf 2`、PTS=1/24〜6/24、B picture / DTS reorder も fixture 検証の対象 |
| 2 | 使用中のdecode/encode/transfer経路を報告する | `media-tests2.log` の上記 seek test と `software_codecs_encode_and_missing_hardware_is_typed` が software decode / AV1 / ProRes の report を確認。`hardware2.json` は H.264 / HEVC の VideoToolbox hardware encode と software decode の codec 名・execution・pixel format・transfer_path・転送 counters を記録。数値は末尾に記載 |
| 3 | LGPL構成のFFmpegを動的リンクし、検出したcodec/hwaccelをcapabilitiesへ報告する | `build.log` / `verify.log` の同梱 LGPL build 検証、`capabilities.json` の FFmpeg 9.0.2、全4 libav library の `LGPL version 2.1 or later` / `--disable-gpl --disable-nonfree --enable-shared --disable-static`、`substituted=true` / `distribution_eligible=true` / `development_only=false`、検出 codec と `hwaccels=[videotoolbox]` を確認。`avcodec-linkage.txt` は prefix 内の native dependencies、`@rpath/libSvtAv1Enc.4.dylib`、system library / frameworks のみで Homebrew / GPL library がないことを確認。`media-tests2.log` の `loaded_libraries_report_configuration_and_override_errors` / `runtime_environment_override_is_used_without_fallback` / `videotoolbox_hybrid_registration_is_hardware_capable` と `service-tests2.log` の `media_wire_roundtrips_and_capabilities_are_shared` が成功。compiled hwaccel の検出と実機 device の成功は区別する |
| 4 | 素材を相対パス・絶対パスの順に解決してhashを照合し、ASSET_MISSING/ASSET_HASH_MISMATCHを報告する | `media-tests2.log` の `relative_first_absolute_fallback_mismatch_missing_and_reverify` が成功。relative 優先、relative 欠落時のみ absolute、同サイズ内容変更の再検証で `ASSET_HASH_MISMATCH`、両方欠落時の `ASSET_MISSING`、unsafe relative の拒否を確認 |
| 5 | asset.relinkがhash一致のファイルだけを再リンクし、project.collectが相対パスのフォルダを書き出す | `media-tests2.log` の `relink_matches_hash_and_never_changes_original_on_failure` / `collect_relative_paths_is_portable_and_failures_leave_no_output`、`service-tests2.log` の `shared_relink_revision_and_portable_sqlite_collect` が成功。hash 一致だけの更新、stale revision 拒否、実 SQLite project copy、directory 移動・元素材削除後の相対パス解決、absolute 削除、既存 output / writer failure を確認 |
| 6 | ソフトウェアエンコードはAV1とProResを提供し、H.264/HEVCのエンコーダーがない環境ではENCODER_UNAVAILABLEを返す | `media-tests2.log` の `software_codecs_encode_and_missing_hardware_is_typed` が LGPL prefix の AV1 / ProRes を各4 frames 実エンコードし全 PTS を roundtrip。hardware 登録を除去した capabilities を注入し H.264 / HEVC の `ENCODER_UNAVAILABLE` を確認。`injected_capabilities_select_videotoolbox_and_reject_ineligible_encoders` も成功し libx264 / libx265 を選択しないことを確認。`build.log` / `verify.log` は必須 AV1 / ProRes の検証成功、`hardware2.json` は encoder のある Apple M1 で両 hardware encode の成功を記録 |
| 7 | 同梱用FFmpegのビルドスクリプトとnative dependencies manifestを管理する | 正本は `scripts/build_ffmpeg_lgpl.py` / `scripts/native-dependencies.json`。`build.log` / `verify.log` の実ホスト build / `--verify-only` が成功し、source SHA-256・configure・license・共有 library hash を `target/native/ffmpeg-lgpl/build-receipt.json` に保存。先行作業の `python3 -m unittest discover -s scripts/tests` は18 passed で、`scripts/tests/test_media_build.py` の manifest / pinned source failure / distribution verification failure を含む。今回の文書更新では build と Python unit tests を再実行していない |

追加: `native_hdr_preserves_ten_bit_planes_and_tags` は PQ / HLG の 10-bit plane、BT.2020 / transfer / matrix / range、ramp 64 / 940 を確認。`rejects_invalid_pts_alpha_and_preserves_output` は PTS tick 不一致、alpha、byte length を拒否。`asset_schema_roundtrip_preserves_future_fields_and_duplicate_ids_fail` は公開 model の Asset と未知 object の lossless preservation を確認する。`stream_metadata` は同じ rational time_base と source tags を返す。

```sh
python3 scripts/fixtures.py generate
python3 scripts/fixtures.py check --generated target/fixtures/generated
python3 -m unittest discover -s scripts/tests -v
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_
python3 scripts/backlog.py check
git diff --check
```

CLI は tagged service Request を stdin / `--request-json` から受ける。例:

```sh
cargo run -p kronello-cli --locked -- --request-json '{"operation":"capabilities.get"}'
```

`asset.relink` payload は `project`, `base_revision`（decimal string）, `asset`（UUID）, `search_directory`。`project.collect` は `project`, `output_directory`。入口別に state を作らない。

## 公開 API と errors

- model: `Project.assets`, `Asset`, `AssetLocator`, `AssetKind`, `StreamMetadata`。JSON schema は Rust から生成し、既存 schema-match test で一致を確認する。
- render: `DecodedVideoFrame`, `VideoDecodeBackend`。pixel は source encoded native plane の alignment=1 packed bytes。working-space linear / premultiplied frame と区別する。
- media: `MediaRuntime::{load,load_directory,capabilities,open_video,encode_video,encode_video_with_capabilities}`, `VideoDecoder::{decode_at,stream_metadata,path_report}`, `resolve_asset`, `relink_asset`, `collect_project`, `content_hash`, `MediaCapabilities::{verify_distribution,select_encoder}`。
- capabilities schema_version=1: `ffmpeg_version`, `library_directory`, `substituted`, `libraries[{name,version,license,configuration}]`, `distribution_eligible`, `development_only`, `codecs[{name,encoder,decoder,hardware}]`, `hwaccels`。
- typed codes: `ASSET_MISSING`, `ASSET_HASH_MISMATCH`, `ENCODER_UNAVAILABLE`, `FFMPEG_UNAVAILABLE`, `DECODE_ERROR`, `ENCODE_ERROR`, `FRAME_NOT_FOUND`, `INVALID_MEDIA_INPUT`, `OUTPUT_EXISTS`, `DISTRIBUTION_LICENSE_ERROR`, `MEDIA_IO_ERROR`, `TIME_ERROR`。service persistence は既存 `REVISION_CONFLICT` 等を維持する。
- `MediaError::EncoderUnavailable { encoder, reason, ffmpeg }` は検出失敗時に `ffmpeg=None`、native 初期化失敗時に `Some(FfmpegErrorDetail { code, operation, message })` を返す。`code` は FFmpeg の元の負の戻り値、`message` は loaded `av_strerror` の説明。`avcodec_open2` の `operation` は codec 名・pixel format・寸法・time_base を含む。`encoder_open_failure_preserves_ffmpeg_code_operation_and_message` は実際の ProRes 初期化失敗を private 境界で hardware として扱い、GPU 不要で診断の保持を確認する。
- `injected_capabilities_select_videotoolbox_and_reject_ineligible_encoders` は native library / hardware 不要の unit test。H.264 / HEVC ごとに VideoToolbox 登録の欠落、decoder のみ、hardware flag なしを注入し、`ENCODER_UNAVAILABLE` と `ffmpeg=None` を確認する。eligible 登録の選択と libx264 / libx265 の非選択も確認する。
- `native_probe_recognizes_hardware_and_hybrid_capability_bits` は codec 列挙と同じ C predicate を直接呼ぶ unit test。HARDWARE 単独、HYBRID 単独、両方、無関係な DELAY bit、無 flag を比較する。FFmpeg library のロードと device の初期化を必要とせず、HYBRID を判定から外す旧実装を検出する。

## VideoToolbox 検出不具合

当初の host 実行は `EncoderUnavailable("H264")` で失敗した。同じ LGPL prefix の FFmpeg CLI は `h264_videotoolbox` / `hevc_videotoolbox` を列挙し、`-allow_sw 0` でエンコードできた。

原因は encoder の登録を `AV_CODEC_CAP_HARDWARE` だけで判定したこと。FFmpeg 9.0.2 の `libavcodec/videotoolboxenc.c` は両 encoder を `AV_CODEC_CAP_HYBRID` として登録するため、Rust の `select_encoder` が native open より前に拒否した。修正後は両 flag を認識し、`allow_sw=0` で hardware を要求する。`allow_sw=1` は FFmpeg 内部の software fallback を許すため採用しない。登録の検出は device の成功を保証せず、実際の open failure は上記の FFmpeg 診断を返す。

完了記録の更新時に `git diff -- crates/kronello-media` を確認したが、crate 全体が未追跡のため tracked diff は空だった。実ファイルの `km_codec_hardware_capable` が HARDWARE / HYBRID の両方を判定し、codec 列挙がその predicate を使うこと、`km_encoder_open` が `allow_sw=0` と YUV420P（ProRes は YUV422P10LE）を指定することを直接確認した。修正後の `hardware2.json` / `hardware2.stderr` は H.264 / HEVC とも正常終了を示す。この文書更新で実装コードは変更していない。

## 先行作業の実施記録

2026-10-04、branch `m2-nle-cli-mcp`、HEAD `71f43c0ccd4f41772e5e6a222f8022e9d7f97f26` と未コミット差分を再検証した。継続開始時に HARDWARE / HYBRID の検出修正、構造化 FFmpeg 診断、native predicate / encoder 選択 / 診断の unit test はすでに残っており、内容を再確認して維持した。hardware example の拡張子を実際の MP4 container に合わせ、CI の重複した FFmpeg インストールを整理した。開発用 Homebrew FFmpeg 9.0.2（GPL configuration）での通常テストと、配布用 LGPL prefix の host build / hardware acceptance を分けて記録する。

この継続作業の sandbox で実行した結果:

| コマンド | 結果 |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0。`target/media-001-host/continuation-clippy-20261004.log` |
| `cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_` | exit 0、288 passed、0 failed / ignored、4 filtered。GPU crate / FrameBridge と `gpu_` test は実行対象外。`target/media-001-host/continuation-cpu-tests-20261004.log` |
| `python3 -m unittest discover -s scripts/tests` | exit 0、18 passed |
| `python3 scripts/fixtures.py check --generated target/fixtures/generated` | exit 0、16 fixture entries / 9 scenes / 28012 bundled bytes |
| `python3 scripts/backlog.py check` | exit 0、60 tasks |
| `git diff --check` | exit 0 |

media crate は unit 3 / assets 4 / media 7、service の media integration は 2 tests が成功した。native predicate と probe 選択の unit test は native runtime を load せず、診断の unit test は ProRes の実際の `avcodec_open2` 失敗を用いる。いずれも VideoToolbox device / Metal を必要としない。toolchain は Rust 1.95.0 / Python 3.14.7。先行する 2026-10-03 の `continuation-cpu-tests.log`（287 passed）は過去の記録として保持し、今回の結果とは区別する。

supervisor が先行して取得した Apple M1 の記録は `target/media-001-host/` の `build.log` / `verify.log` / `fixtures.log` / `capabilities.json` / `media-tests.log` / `service-tests.log`。LGPL build と verification、fixture generate / check、software codec / service tests が成功していた。先行した `hardware.stderr` の `EncoderUnavailable("H264")` は修正前の失敗記録として保持する。同梱 prefix の receipt は `target/native/ffmpeg-lgpl/build-receipt.json`。

先行作業では host の修正後検証として `bash target/media-001-host/rerun-videotoolbox.sh` を supervisor に依頼する手順を用意し、結果未受領の時点では MEDIA-001 を `in_progress` に維持した。その後、既存 LGPL prefix と `target/media-lgpl` を再利用したホスト結果を受領した。完了更新の根拠は末尾の「ホスト検証と完了記録」に記載する。

Linux CI は `libavcodec-dev libavformat-dev libavutil-dev libswscale-dev` をインストールし、`cargo test --workspace --locked` で media を含む全 crate を実行する。media の欠落を skip / ignored / 成功扱いにする分岐はない。Linux CI runner の実行はこの作業環境で確認できないため、workflow の確認と local macOS 実行を区別する。

## 今回の crate 単位の再検証

2026-10-04、同じ HEAD と未コミット差分を main checkout で確認した。上の workspace コマンドは先行作業の記録であり、今回の delegated job では実行していない。`ffi.rs`、native predicate、構造化診断、`hardware_roundtrip.rs` の MP4 拡張子と失敗の伝播に不整合・重複を認めず、変更せず維持した。service の既存差分は共有 Request / ResultData と dispatch / wire の追加、および独立した media module / tests に限定され、今回 service は変更していない。

今回の Cargo コマンドには `CARGO_HOME=/private/tmp/kronello-media-cargo` を指定した。開発用 system FFmpeg を使用し、VideoToolbox / Metal の device acceptance は実行していない。

| 今回実行したコマンド | 結果 |
|---|---|
| `cargo check -p kronello-media --all-targets --locked` | exit 0 |
| `cargo test -p kronello-media --lib --locked` | exit 0、3 passed |
| `cargo test -p kronello-media --locked` | exit 0、unit 3 / assets 4 / media 7 passed、0 failed / ignored。doc-test 0 |
| `cargo test -p kronello-service --test media --locked` | exit 0、2 passed、0 failed / ignored |
| `cargo fmt --all --check` | exit 0 |
| `python3 -m unittest discover -s scripts/tests` | exit 0、18 passed |
| `python3 scripts/fixtures.py check --generated target/fixtures/generated` | exit 0、16 entries / 9 scenes / 28012 bundled bytes |
| `python3 scripts/backlog.py check` | exit 0、60 tasks |
| `git diff --check` | exit 0 |

## ホスト検証と完了記録

2026-10-04 04:55 JST 頃、supervisor が Apple M1 / macOS 27.0、main checkout の branch `m2-nle-cli-mcp`（HEAD `71f43c0ccd4f41772e5e6a222f8022e9d7f97f26` と未コミット実装）で実行した結果を受領した。以下は supervisor 提供のホスト測定であり、今回の文書更新 job が実行したテストではない。実在するログ・JSON と報告内容を照合した。証拠は `target/media-001-host/` に保存されている。

実行環境は `PKG_CONFIG_PATH=$PWD/target/native/ffmpeg-lgpl/lib/pkgconfig`、`KRONELLO_FFMPEG_LIB_DIR=$PWD/target/native/ffmpeg-lgpl/lib`、`PATH=$PWD/target/native/ffmpeg-lgpl/bin:$PATH`、`CARGO_TARGET_DIR=$PWD/target/media-lgpl`。修正後の検証は既存 LGPL prefix を再利用した。

| ホスト実行コマンド | 結果・証拠 |
|---|---|
| `cargo test -p kronello-media --locked` | exit 0。unit 3 / assets 4 / media 7 passed、0 failed / ignored、doc-test 0。`media-tests2.log` |
| `cargo test -p kronello-service --test media --locked` | exit 0。2 passed、0 failed / ignored。`service-tests2.log` |
| `cargo run -p kronello-media --example hardware_roundtrip --locked -- target/media-lgpl/hardware-roundtrip2` | exit 0。H.264 / HEVC の各4 frames を hardware encode し、software decode で全 PTS と寸法を確認。`hardware2.json` / `hardware2.stderr` |
| `otool -L target/native/ffmpeg-lgpl/lib/libavcodec.63.dylib` | prefix 内の `libswresample.7` / `libavutil.61` / `libdav1d.7`、`@rpath/libSvtAv1Enc.4`、`/usr/lib/libSystem.B.dylib` と system frameworks にリンク。Homebrew / GPL library は含まれない。`avcodec-linkage.txt`。これは当該 libavcodec の依存確認であり、全 release package の再配置・署名検証ではない |
| 同梱 LGPL build と `python3 scripts/build_ffmpeg_lgpl.py --verify-only`（先行ホスト実行） | 成功。双方のログ末尾に `verified LGPL shared libraries, AV1 and ProRes`。`build.log` / `verify.log`。build の jobs 指定は今回のホスト報告では未提供 |
| `python3 scripts/fixtures.py generate`、`python3 scripts/fixtures.py check --generated target/fixtures/generated`（先行ホスト実行） | 成功。9 media 生成・decode、16 entries / 9 scenes 検証。`fixtures.log` |
| `cargo run -p kronello-media --example capabilities --locked -- --verify-distribution`（先行ホスト実行） | 成功。FFmpeg 9.0.2、全4 library の LGPL license / GPL・nonfree 無効、`substituted=true` / `distribution_eligible=true`、codec / hwaccel を確認。`capabilities.json` |

`hardware2.json` の H.264 encode は `h264_videotoolbox` / `execution=hardware`、入力 `rgba` →出力 `yuv420p`、`transfer_path=cpu_rgba_to_hardware_encoder`。logical counters は `cpu_conversion_input_bytes=65536`、`cpu_conversion_output_bytes=24576`、`cpu_upload_bytes=24576`。decode は `h264` / `execution=software`、`yuv420p`、`software_decode_to_cpu_native_planes`、`cpu_copy_bytes=79872`。HEVC も `hevc_videotoolbox` による hardware encode / `hevc` による software decode と同じ形式・転送 counters を記録する。driver 内部の転送・待機測定や hardware decode / GPU 常駐成功を表す数値ではない。

上の7条件への証拠対応を記録し、MEDIA-001 を `done` に更新した。Linux CI runner、hardware decode / GPU 常駐 integration、working-space color 変換、全 release package の検証は今回のホスト結果には含まれない。今回の変更は `docs/testing/media-001.md`、MEDIA-001 の status のみを変更した `docs/backlog/backlog.json`、そこから再生成した `docs/backlog/BACKLOG.md` に限定する。

今回の文書更新 job で実行した検証は `python3 scripts/backlog.py render`（exit 0）、`python3 scripts/backlog.py check`（exit 0、60 tasks）、`cargo fmt --all --check`（exit 0）、`git diff --check`（exit 0）。crate tests / hardware example / workspace test / Clippy は今回再実行していない。
