# RELEASE-001 の検証

対象: CLI / MCP と LGPL FFmpeg runtime の macOS 配布 directory。[ADR-0065](../adr/0065-relocatable-macos-distribution.md) と [12 プラットフォームと依存](../architecture/12-platform-dependencies.md) に従う。GUI app bundle、Metal / VideoToolbox、Windows / Linux package の受け入れはこの結果に含めない。

## 配置と固定 input

```text
package/
  bin/kronello
  bin/kronello-mcp
  lib/libavutil.61.dylib
  lib/libavcodec.63.dylib
  lib/libavformat.63.dylib
  lib/libswscale.10.dylib
  lib/libswresample.7.dylib
  lib/libSvtAv1Enc.4.dylib
  lib/libdav1d.7.dylib
  licenses/LICENSE-MIT
  licenses/LICENSE-APACHE
  licenses/ffmpeg/COPYING.LGPLv2.1
  licenses/svt-av1/{LICENSE.md,LICENSE-BSD2.md,PATENTS.md}
  licenses/dav1d/COPYING
  tools/release_roundtrip
  tools/build_ffmpeg_lgpl.py
  tools/macos_release.entitlements.plist
  native-dependencies.json
  native-build-receipt.json
  build-provenance.json
  package-manifest.json
```

FFmpeg 9.0.2 / SVT-AV1 4.2.0 / dav1d 1.5.4 と source URL / archive SHA-256 / configure は `scripts/native-dependencies.json` が正本。パッケージ作成は native prefix の manifest と receipt、元 shared library hashes、SVT / dav1d の実版、license hashes を照合する。さらに `--sources` directory の source archives を pinned hash で検証し、license と PATENTS の byte 内容を archive 原文と比較する。development system FFmpeg、不要な libavdevice / libavfilter、FFmpeg executable、headers、pkg-config、static archive は同梱しない。コピー済み native prefix は configuration に記録された元 build prefix からの参照も閉じた `lib/` 内へ書き換える。

FFmpeg 本体を Rust に静的リンクしない。library の ID は `@rpath/<name>`、内部依存 / rpath は `@loader_path`、executable の依存 / rpath は `@executable_path/../lib`。すべての Mach-O に `lipo -archs` と `otool -L/-l`、全7 library に `otool -D` を実行する。Apple `/usr/lib/` と `/System/Library/Frameworks/` 以外の absolute dependency、外部 rpath、期待しない Mach-O、欠落 library、host architecture 不一致は失敗にする。

`package-manifest.json` がある executable は root の `lib/` を既定ロードする。`KRONELLO_FFMPEG_LIB_DIR` は lib directory そのものを指定する override。指定の欠落・ABI 不一致で開発 prefix / system へ戻らない。将来 GUI app はこの directory 全体を Resources 内へ内包できるが、FFI host は同じ `lib/` を明示して GUI 自身の再配置・署名を検証する。`scripts/build_macos_app.py` / GUI bundle は変更していない。

## 再現手順

macOS、Rust 1.95.0、Python 3.12 以上、C/C++ compiler、pkg-config、CMake / Meson / Ninja / make、Xcode command-line tools が必要。共有環境の `CARGO_HOME` / `CARGO_TARGET_DIR` / `TMPDIR` を維持し、jobs は3に制限する。出力 directory と report は新規 path のみ。失敗した候補を次の成功に流用せず、新規名で再実行する。

native prefix がなければ次を実行する。これは **pending host run**。期待結果は exit 0、5 library の LGPL / ABI、SVT / dav1d の版、AV1 / ProRes / PCM24、license hashes を含む receipt。

```sh
python3 scripts/build_ffmpeg_lgpl.py --prefix target/native/ffmpeg-lgpl --jobs 3
```

既存 prefix を再利用する場合は次で receipt を現行形式へ更新する。4 本だけの古い receipt は package 入力として拒否する。古い manifest の表示文だけが現行版と異なる場合も自動書換えせず、source / configure / hashes の一致を確認して supervisor が更新する。

```sh
python3 scripts/build_ffmpeg_lgpl.py --prefix target/native/ffmpeg-lgpl --verify-only
```

署名を含む以下は **pending host run**。package script は manifest / hashes を確認後、pinned headers の `PKG_CONFIG_PATH` / `PKG_CONFIG_LIBDIR` と jobs=3 で release CLI / MCP / acceptance executable をビルドする。Cargo artifact JSON から shared target directory の実パスを取得し、Mach-O に install-name 変更用 header space を確保する。source がビルド中に変化したら成功 inventory を作らない。

```sh
python3 scripts/package_macos.py \
  --prefix target/native/ffmpeg-lgpl \
  --sources target/native/downloads \
  --output target/release-001/adhoc-package
python3 scripts/verify_package.py \
  --package target/release-001/adhoc-package \
  --relocated "$TMPDIR/kronello-release-001-adhoc-relocated" \
  --report target/release-001/adhoc-verification.json
```

期待結果は両 command が exit 0。package は install name / rpath の変更後に全7 library → 3 executable の順に ad-hoc 署名し、各ファイルを `codesign --verify --deep --strict --verbose=2` で確認する。verifier は package と元 native prefix の外にコピーし、署名を再確認する。report の `status=passed` と `acceptance_verified=true`、全 command の期待 exit code、5 本の loaded library の ABI / version / license / configuration、`distribution_eligible=true` / `development_only=false`、実際の relocated `library_directory` を確認する。

verifier は独立 cwd と清掃した環境（`DYLD_*` / `KRONELLO_*` / `PKG_CONFIG_*` 除去）で、実 CLI の `capabilities.get` と MCP initialize / tools/call を起動する。`release_roundtrip` は GPU を要求せず、64×64、24 fps、4 frame の AV1 と ProRes を encode / seek decode し全 PTS / end / dimension、codec とゼロ開始・1/6秒 duration を確認する。frame ごとに異なる一様 RGB を使い、BT.709 / limited-range tag と、独立した BT.709 参照式に対する全 YUV sample の誤差を確認する。許容値は8-bit sample単位で4（10-bit sample は4で割って比較）。AV1 / ProRes の逆順・反復 seek と mux 後の ProRes で合計104,448 samplesを比較し、最大誤差を report に保存する。同じ frame の繰り返し、空の plane、壊れた pixel、誤った tag は成功にしない。8,000 stereo frames の PCM24 を ProRes と MOV に mux し、probe / sample count / zero origin / 全16,000 channel samples の誤差 `<= 1/8388608` を検証する。これは media library の software export 経路であり、共有 service の AV1 job profile を新設するものではない。

別 directory に全7 library の同一 ABI copy を作り、copy を ad-hoc 署名し直して override の `substituted=true` を CLI と MCP で確認し、codec 往復も再実行する。任意の第三者・改変版すべての互換性を保証するものではない。存在しない override と、package manifest を残して `libswresample` を削除した copy は、実 CLI の exit 1 / `FFMPEG_UNAVAILABLE` を要求する。

report / codec outputs / replacement / broken-package は配布物外の `<report-stem>-artifacts/` に保存する。report は platform / architecture / UTC、revision、dirty status / tracked diff SHA-256 / source file hashes、配布後 inventory の `package_sha256`、`manifest_sha256`、実 command / cwd / exit code / stdout / stderr を持つ。`package_sha256` は canonical JSON の file inventory（relative path、byte hash、size、mode）の SHA-256 であり、ZIP の hash ではない。manifest 自体は別 hash で識別する。失敗した report は `status=failed` / `acceptance_verified=false`。`--static-only` の exit 0 は `static-passed` に限り、署名 / 起動 / roundtrip の代替にしない。

## Developer ID / notarization / Gatekeeper

以下は credential を持つ supervisor の手動工程であり、**pending host run**。既定の ad-hoc package に対して Developer ID を後付けすると inventory が変わるため、別の新規 package を作成する。

```sh
python3 scripts/package_macos.py \
  --prefix target/native/ffmpeg-lgpl --sources target/native/downloads \
  --output target/release-001/developer-id-package \
  --sign-identity 'Developer ID Application: NAME (TEAMID)'
python3 scripts/verify_package.py \
  --package target/release-001/developer-id-package \
  --relocated "$TMPDIR/kronello-release-001-developer-id-relocated" \
  --report target/release-001/developer-id-verification.json
ditto -c -k --keepParent target/release-001/developer-id-package target/release-001/kronello-macos.zip
shasum -a 256 target/release-001/kronello-macos.zip
xcrun notarytool submit target/release-001/kronello-macos.zip --keychain-profile KronelloNotary --wait
```

script は secrets を受け取らず、notarytool を起動しない。profile の作成、Developer ID identity の指定、upload は host 側で行う。期待結果は署名と完全 verifier が exit 0、notary submission が `Accepted`。公開用 archive の hash、submission ID、platform / revision、全 command / exit code を別途記録する。

Developer ID executable は timestamp / Hardened Runtime を使い、FFmpeg 差し替えを許す `com.apple.security.cs.disable-library-validation` だけを付ける。[Apple の説明](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.cs.disable-library-validation)では通常の Library Validation は Apple または同じ Team ID の library に制限される。本 verifier は replacement の全7 library を ad-hoc 署名し直し、Developer ID executable からその copy を override でロードして capabilities / roundtrip を検証する。同じ Team ID の copy だけの成功を、異なる署名の差し替え検証として報告しない。

ZIP は staple の対象にしない。GUI `.app` / `.dmg` / installer の staple は将来の該当配布工程で実施する。notary acceptance 後、quarantine を保持する実際のダウンロード経路から独立 path に展開し、Gatekeeper の assessment と CLI/MCP の実起動を記録する。ホスト環境・配布形式に応じた `spctl --assess --type execute --verbose=4 <binary>` の結果と実起動を併記し、ad-hoc または単独の codesign 成功を Gatekeeper 成功へ読み替えない。[Apple の notarization 手順](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)に従う。

## 前回の interrupted job が残した実施記録

2026-10-05、`m3-release`、開始時 HEAD `4728b6120faf90b4d197e46b5a8850b6fefd3697` と未コミット実装。sandbox は macOS 27.0 / arm64、Rust 1.95.0、Python 3.14.7。GPU と hardware codec、host signing command の capability はない。共有 Cargo cache / target と managed TMPDIR を使用した。元 checkout の cached LGPL prefix と source archives を本 worktree の `target/native/` にコピーし、元 checkout 自体は変更していない。local copy の manifest 表示文を現行版に揃えて receipt を更新した。fresh native build は実行していない。

| 条件 | 実装と証跡 | 状態 |
|---|---|---|
| 1. 同梱 FFmpeg / SVT / dav1d、license / PATENTS / source manifest を固定し GPL/nonfree と development library を拒否 | 5 本を含む native receipt、upstream archives との license byte 比較、元 library hashes と配布後 inventory、全10 Mach-O 走査。Python 拒否テストは swresample ABI/GPL/nonfree、dav1d の Homebrew dependency、rpath、欠落 / 追加 Mach-O、tamper を確認 | native receipt 更新と CPU/static 検査を実施。署名済み配布物の最終検証は pending host run |
| 2. 元 prefix 外へ再配置し5 libraryの link / ABI / replaceability / capabilities、ProRes/PCM24 と AV1 往復 | verifier が独立 path と cwd で CLI / MCP、loaded5本、override と失敗、`release_roundtrip` の video / PCM / mux を検証 | スクリプトと拒否テストを実装。署名後の完全 relocation / roundtrip は pending host run |
| 3. 署名・配布手順、実 binary 起動、platform / revision / package hash / command / exit code | 内側から署名、配布後 hash、常に残る verification report、Developer ID / notarization の分離 | ad-hoc / Developer ID / notarization / Gatekeeper は pending host run。Windows / Linux package は未検証 |

以下は前回の interrupted job が残した sandbox 実行記録であり、今回の再開 job が実行した検証でも、supervisor から受領した host 測定でもない。元ログと candidate は保持する。今回の再開結果は末尾に分けて記録する。Cargo は `CARGO_BUILD_JOBS=3`。clippy / test はディスク使用を抑えるため `CARGO_PROFILE_DEV_DEBUG=0`、test は加えて `CARGO_PROFILE_TEST_DEBUG=0` を指定した。debug assertions は維持される。

| 実行 command | 結果・証跡 |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo check -p kronello-media --example release_roundtrip --locked` | exit 0 |
| `cargo clippy -p kronello-media -p kronello-cli -p kronello-mcp --all-targets --locked -- -D warnings` | exit 0。`target/release-001-clippy.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0。`target/release-001-workspace-clippy.log` |
| `python3 -m unittest discover -s scripts/tests -v` | exit 0、27 passed / 2 skipped（既存の opt-in integration 2 件）。新規 release checks 8 件、同一 ABI の別 FFmpeg release 拒否 1 件を含む。`target/release-001-python-tests-pinned.log` |
| `python3 scripts/build_ffmpeg_lgpl.py --prefix target/native/ffmpeg-lgpl --verify-only` | exit 0。FFmpeg 9.0.2 と5本の ABI / LGPL、SVT 4.2.0、dav1d 1.5.4、PCM24 を含む必須 codec、license hashes の receipt を local copy に保存 |
| `python3 scripts/package_macos.py --prefix target/native/ffmpeg-lgpl --output target/release-001/unsigned-pinned --unsigned` | exit 0。source archive hash / license 原文、元 library hash、全10 Mach-O を検査。`target/release-001-package-pinned.log`、package 内の `build-provenance.json` |
| `python3 scripts/verify_package.py --package target/release-001/unsigned-pinned --relocated "$TMPDIR/kronello-release-001-static-pinned" --report target/release-001/static-pinned-verification.json --static-only` | exit 0、`static-passed` / `acceptance_verified=false`。10 Mach-O / 37 commands の全 exit code が0 |
| `python3 scripts/verify_package.py --package target/release-001/unsigned-pinned --relocated "$TMPDIR/kronello-release-001-pinned-unsigned-rejected" --report target/release-001/pinned-unsigned-rejection.json` | 期待どおり exit 1。`unsigned assembly cannot pass release verification`、report は `failed` / `acceptance_verified=false`。codesign / runtime 検査へ進まない |
| `KRONELLO_FFMPEG_LIB_DIR="$PWD/target/native/ffmpeg-lgpl/lib" "$CARGO_TARGET_DIR/release/examples/release_roundtrip" target/release-001/native-roundtrip` | exit 0。AV1 / ProRes の PTS / end、ProRes+PCM24 mux、全16,000 channel samples を検査。`target/release-001-native-roundtrip.json` / `.stderr`。書換え前の Cargo executable と未再配置の native runtime を使った CPU 検査であり、署名済み package の実起動ではない |
| `PATH="$PWD/target/native/ffmpeg-lgpl/bin:$PATH" python3 scripts/fixtures.py generate` | exit 0。9 media を生成・decode。`target/release-001-fixtures.log` |
| `PATH="$PWD/target/native/ffmpeg-lgpl/bin:$PATH" python3 scripts/fixtures.py check --generated target/fixtures/generated` | 初回 exit 1（外部 NotoSans font 欠落）。既存の font cache を本 worktree にコピー後、再実行 exit 0、16 entries / 9 scenes |
| `cargo test -p kronello-media --locked`（runtime override と pinned FFmpeg PATH を指定） | exit 0、unit 3 / assets 4 / audio 8 / media 7、合計22 passed、0 failed / ignored / filtered。`target/release-001-media-tests.log` |
| `cargo test -p kronello-media -p kronello-cli -p kronello-mcp --locked -- --skip gpu_`（同じ native 環境） | 2 回とも exit 101。CLI は各回 unit/job 17 + machine 20 passed、GPU 1 filtered。MCP は各回11 passed / 1 failed / 1 filtered、media に到達せず。初回は API fixture の `template.migration_plan` が `STORAGE_ERROR: disk I/O error`、2回目は同じ fixture が MCP response timeout。`target/release-001-rust-tests.log` / `target/release-001-rust-tests-rerun.log` |
| `cargo test -p kronello-mcp --test stdio --locked public_api_fixture_commands_return_schema_valid_success_from_real_binary -- --exact`（同じ native 環境） | exit 0、1 passed / 12 filtered。`target/release-001-mcp-isolated.log` |
| `cargo test -p kronello-mcp --locked -- --test-threads=1`（同じ native 環境） | exit 0、13 passed / 0 failed / ignored / filtered。`target/release-001-mcp-serial.log`。GPU absence の negative test は test-only injection であり device 実測ではない |
| `git diff --check` | exit 0 |

未署名候補 `target/release-001/unsigned-pinned` の inventory SHA-256 は `571fe8a0de625730b0927dbce0c230b0412b8b1ce27831e9a951708844ac6aa3`、manifest SHA-256 は `0aea21b064297339253584efc6f088d018f23b9b785001efb504f4c380dc3a91`。これは当該実行時の dirty source snapshot と package の識別であり、署名済み公開 artifact の hash ではない。最終結果の本文追記後も binary / packaging code は変更していない。

前回の MCP suite 失敗の原因は特定していない。前回の単独 / serial の成功を、並行 suite の成功へ読み替えない。service / store / MCP transport / 既存 stdio tests の実装は変更していない。再開 job の serial 実行でも timeout が再現したため、並行実行だけが原因とは扱わない。調査は supervisor に引き継ぐ。

署名後の完全 relocation / replacement / 実 CLI/MCP 起動と roundtrip、Developer ID / notarization / Gatekeeper は **pending host run**。GPU を含む `CARGO_BUILD_JOBS=3 cargo test --workspace --locked` も今回実行していない。必要な native 環境を指定して host で実行する。RELEASE-001 の `done` 判定は supervisor が host report を確認して行う。禁止された backlog / ADR index / open questions / docs index は変更していない。

## 再開 job の検証（2026-10-05）

開始時 HEAD は `4728b6120faf90b4d197e46b5a8850b6fefd3697`、branch は `m3-release`。前回の未コミット実装と `target/native/` / `target/release-001/` をレビューして保持した。今回も macOS 27.0 / arm64、Rust 1.95.0、Python 3.14.7、共有 `CARGO_HOME` / `CARGO_TARGET_DIR` と managed `TMPDIR`、jobs=3 を使用した。fresh native build、GPU / hardware codec、codesign は実行していない。supervisor 提供の新しい host 測定も受領していない。

今回補った内容は、全 architecture を明示する `otool -arch all`、実 cwd と timeout 時の途中出力の保存、ビルド前後の revision / source file list / dirty status の一致確認、frame ごとに異なる RGB の全 YUV sample 比較、古い「verify-only は4本」の説明と検証成功メッセージの修正。前回の package layout / 署名順 / loader と override / GPL・nonfree 拒否の実装を維持した。

下記の結果は今回の job が実際に実行したもの。ログは `target/release-001-resumed/` 配下。Cargo の clippy / test は `CARGO_PROFILE_DEV_DEBUG=0`、test は加えて `CARGO_PROFILE_TEST_DEBUG=0`。すべて jobs=3。native headers は `PKG_CONFIG_PATH` / `PKG_CONFIG_LIBDIR` に `$PWD/target/native/ffmpeg-lgpl/lib/pkgconfig` を指定し、codec tests は `KRONELLO_FFMPEG_LIB_DIR=$PWD/target/native/ffmpeg-lgpl/lib` と native FFmpeg の `PATH` を使用した。CLI / MCP tests の state root は managed TMPDIR 内に明示した。

| command | 結果・証跡 |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0。`workspace-clippy.log` |
| `python3 -m unittest discover -s scripts/tests -v` | exit 0、28 passed / 2 skipped（既存 opt-in integration）。release checks は9件。`python-tests-final.log` |
| `python3 scripts/build_ffmpeg_lgpl.py --prefix target/native/ffmpeg-lgpl --verify-only` | exit 0、5本の LGPL / ABI、FFmpeg 9.0.2、SVT 4.2.0 / dav1d 1.5.4、AV1 / ProRes / PCM24 の receipt を local prefix に更新。`native-verify.log` |
| `python3 scripts/fixtures.py check --generated target/fixtures/generated`（native FFmpeg PATH） | exit 0、16 entries / 9 scenes。`fixtures-check.log`。fixture の再生成は今回実行していない |
| `cargo test -p kronello-media -p kronello-cli -p kronello-mcp --locked -- --test-threads=1 --skip gpu_` | exit 101。CLI jobs 17 / machine 20 passed、GPU 1 filtered。MCP 11 passed / 1 failed / 1 filtered、media に到達せず。既存 `public_api_fixture_commands_return_schema_valid_success_from_real_binary` の20秒 MCP response timeout。`rust-tests.log` |
| `cargo test -p kronello-media --locked`（上記 native 環境） | exit 0、unit 3 / assets 4 / audio 8 / media 7、合計22 passed、0 failed / ignored / filtered。`media-tests.log` |
| `cargo run --release -p kronello-media --example release_roundtrip --locked -- target/release-001-resumed/native-roundtrip`（上記 native headers / runtime） | exit 0。104,448 YUV samples、最大誤差0.42517692878523405（8-bit sample単位、上限4）、16,000 PCM24 channel samples、PTS / end / mux を検証。`native-roundtrip.json` / `.stderr`。未書換え Cargo executable と未再配置 native runtime の CPU 検査であり、配布後の署名・起動検証ではない |
| `cargo test -p kronello-mcp --test stdio --locked public_api_fixture_commands_return_schema_valid_success_from_real_binary -- --exact`（上記 native 環境、release build 完了後） | exit 0、1 passed / 12 filtered、34.72秒。`mcp-isolated.log`。suite 全体の成功には読み替えない |
| `python3 scripts/package_macos.py --prefix target/native/ffmpeg-lgpl --sources target/native/downloads --output target/release-001-resumed/unsigned-package --unsigned` | exit 0。pinned source archive / license byte 比較、native receipt と元 library hash、全10 Mach-O の依存・rpath・ID・architecture、build 前後の source 一致を検査。`package.log` と package の `build-provenance.json` |
| `python3 scripts/verify_package.py --package target/release-001-resumed/unsigned-package --relocated "$TMPDIR/kronello-release-001-resumed-static" --report target/release-001-resumed/static-verification.json --static-only` | exit 0、`static-passed` / `acceptance_verified=false`。全10 Mach-O / 37 commands が exit 0。`static-verification.json` / `.log` |
| `python3 scripts/verify_package.py --package target/release-001-resumed/unsigned-package --relocated "$TMPDIR/kronello-release-001-resumed-unsigned-rejected" --report target/release-001-resumed/unsigned-rejection.json` | 期待どおり exit 1、`unsigned assembly cannot pass release verification`。`failed` / `acceptance_verified=false`。codesign / runtime 起動へ進まない。`unsigned-rejection.json` / `.log` |
| `git diff --check` | exit 0 |

MCP timeout 時には別の release build も実行中だったが、これを原因と断定していない。serial suite の失敗を保存し、単独 fixture の再確認は別の結果として扱う。service / store / MCP transport / 既存 stdio tests は今回変更していない。

今回の未署名候補は inventory の22 files と `package-manifest.json`（計23 files）。inventory SHA-256 は `f89b356e9ff57d057b4b1a12fb130bd09f65640f02bac0b2fda3cbff58bf9159`、manifest SHA-256 は `e39dc052b9e5ec899a6a153481ad6f687131727afb664a3ca0fd23cbbd3acc78`。これは今回組み立てた dirty source snapshot と未署名候補の識別であり、公開用署名済み artifact の hash ではない。package 作成後はこの結果の文書追記だけを行い、binary / packaging code は変更していない。

| 受け入れ条件 | 再開時の証跡と残り |
|---|---|
| 1. pinned native libraries / licenses / PATENTS / manifest、GPL/nonfree・development library 排除 | local receipt 更新、実際の unsigned assembly と再配置先の全10 Mach-O / inventory 検査は成功。署名後の inventory / 最終検証は **pending host run** |
| 2. prefix 外への再配置、全5 FFmpeg library の link / ABI / capabilities / replaceability、AV1 と ProRes/PCM24 往復 | 静的再配置と未再配置 native runtime の CPU roundtrip は成功。署名後の relocated CLI / MCP / acceptance executable の実起動、5本の loaded capabilities、別署名 runtime の override と roundtrip は **pending host run** |
| 3. 署名・配布手順、実 binary 起動、platform / revision / package hash / command / exit | package / verification report の生成と未署名拒否は確認。ad-hoc / Developer ID / notarization / Gatekeeper は **pending host run**。Windows / Linux package は未検証 |

supervisor は上記「再現手順」の fresh ad-hoc package 作成と完全 verifier（期待: 両方 exit 0、`status=passed` / `acceptance_verified=true`）を実行し、必要なら「Developer ID / notarization / Gatekeeper」を別候補で実行する。full workspace test は次の native 環境で host 実行する（期待: exit 0、GPU を含む required tests が成功。今回の filtered tests はその代替としない）。

```sh
CARGO_BUILD_JOBS=3 \
  PKG_CONFIG_PATH="$PWD/target/native/ffmpeg-lgpl/lib/pkgconfig" \
  PKG_CONFIG_LIBDIR="$PWD/target/native/ffmpeg-lgpl/lib/pkgconfig" \
  KRONELLO_FFMPEG_LIB_DIR="$PWD/target/native/ffmpeg-lgpl/lib" \
  PATH="$PWD/target/native/ffmpeg-lgpl/bin:$PATH" \
  KRONELLO_STATE_ROOT="$TMPDIR/kronello-release-001-host-state" \
  cargo test --workspace --locked
```

supervisor 所有の更新は、host report の platform / revision / package hashes / commands / exit codes を本書へ追加、受け入れ確認後だけ RELEASE-001 status の更新と backlog 再生成・check、ADR index への0065登録、必要なら docs index へのリンク。今回禁止されたファイルは未変更で、status は `in_progress` のまま。MCP の20秒 response timeout の調査も引き継ぐ。指示で参照された `scripts/build_macos_app.py` はこの checkout に存在せず、GUI app bundle / `apps/macos/README.md` は変更していない。commit / push / merge は実行していない。

## 最終再開 job の検証（2026-10-05）

開始時に存在した上記実装と検証記録を保持し、この job では結果の文書追記だけを変更した。HEAD / platform / toolchain は上記と同じ。ログは `target/release-001-final/`。以下はこの job が実際に実行した結果であり、前の job の結果や host 測定とは区別する。

Cargo の環境は次のとおり。共有 `CARGO_HOME` / `CARGO_TARGET_DIR` / managed `TMPDIR` を維持した。test と clippy は debug symbol のみ省略し、debug assertions を維持した。crate suite と package の Cargo build は重ねて実行していない。

```sh
export CARGO_BUILD_JOBS=3
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export PKG_CONFIG_PATH="$PWD/target/native/ffmpeg-lgpl/lib/pkgconfig"
export PKG_CONFIG_LIBDIR="$PWD/target/native/ffmpeg-lgpl/lib/pkgconfig"
export KRONELLO_FFMPEG_LIB_DIR="$PWD/target/native/ffmpeg-lgpl/lib"
export PATH="$PWD/target/native/ffmpeg-lgpl/bin:$PATH"
export KRONELLO_STATE_ROOT="$TMPDIR/kronello-release-001-final-state"
```

| command | 結果・証跡 |
|---|---|
| `cargo test -p kronello-media -p kronello-cli -p kronello-mcp --locked -- --skip gpu_` | exit 0。CLI 37、MCP 12、media 22、合計71 passed / 0 failed / 2 filtered。CLI の実 GPU test と MCP の injected GPU absence test が名前で除外された。`rust-tests.log` |
| `cargo fmt --all --check` | exit 0。`fmt.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0。`workspace-clippy.log` |
| `python3 -m unittest discover -s scripts/tests -v` | exit 0。30件中28 passed / 2 skipped（既存 opt-in integration）、release checks 9件。`python-tests.log` |
| `python3 scripts/generate_swift_api.py --check` | exit 0。生成済み Swift transport と committed schema が一致。生成ファイルは変更していない。`swift-api-check.log` |
| `python3 scripts/package_macos.py --prefix target/native/ffmpeg-lgpl --sources target/native/downloads --output target/release-001-final/unsigned-package --unsigned` | exit 0。source archives / license 原文 / native hashes、全10 Mach-O、source snapshot 一致を検査。`package.log` |
| `python3 scripts/verify_package.py --package target/release-001-final/unsigned-package --relocated "$TMPDIR/kronello-release-001-final-static" --report target/release-001-final/static-verification.json --static-only` | exit 0。全10 Mach-O / 37 commands が exit 0。`static-passed` / `acceptance_verified=false` |
| `KRONELLO_FFMPEG_LIB_DIR="$PWD/target/native/ffmpeg-lgpl/lib" "$CARGO_TARGET_DIR/release/examples/release_roundtrip" target/release-001-final/native-roundtrip` | exit 0。104,448 YUV samples、最大誤差0.42517692878523405、16,000 PCM24 channel samples、PTS / end / mux を確認。`native-roundtrip.json` / `.stderr`。未再配置の Cargo executable / native prefix による CPU 検査 |
| `python3 scripts/verify_package.py --package target/release-001-final/unsigned-package --relocated "$TMPDIR/kronello-release-001-final-unsigned-rejected" --report target/release-001-final/unsigned-rejection.json` | 期待どおり exit 1。`unsigned assembly cannot pass release verification`、`failed` / `acceptance_verified=false`。署名・実起動へ進まない |

元の `target/release-001-rust-tests.log` は MCP API fixture の `template.migration_plan` で `STORAGE_ERROR: disk I/O error`、その後のログは20秒 response timeout を記録している。この job の同じ crate suite は成功したが、過去の失敗原因は未特定であり、sandbox や並行 build が原因とは断定しない。test に指定した `KRONELLO_FFMPEG_LIB_DIR` は CLI / MCP child に継承され、`MediaRuntime::load` の既存 `Some(path)` 分岐を通る。今回追加した package の既定検索処理はその分岐では実行されない。store / service / MCP transport / 既存 stdio tests は変更していない。再発時は失敗した suite のログを使って supervisor が調査する。

この未署名候補は22 inventory files + manifest（計23 files）。inventory SHA-256 は `5507539659a0cfee23a68fc33677afcd3904a26c9e932dcf0d68c9f323b828a9`、manifest SHA-256 は `3bdf686d61cda1452c47344aa948fe5cd4763ea72f02d2efe28c5bff70f483ae`。識別対象は package 作成時の dirty source snapshot であり、署名済み公開物ではない。package 作成後の変更はこの文書追記だけで、binary / packaging code は変更していない。

## Supervisor の次の host 実行順

以下はすべて **pending host run**。本 worktree の root で、上記の native 環境を設定し、既存の候補を上書きしない。出力名が存在する場合は新規名を選ぶ。

1. cached prefix がある場合は receipt を再確認する。ない場合だけ「再現手順」の fresh native build を実行し、source archives も `target/native/downloads` に揃える。
2. ad-hoc 署名済み候補を新規作成する。期待: exit 0、全7 library → 3 executable の署名と各 `codesign --verify --deep --strict --verbose=2` が成功。
3. 元 package / 元 prefix の外へ再配置し、完全 verifier を実行する。期待: exit 0、report の `status=passed` / `acceptance_verified=true`。静的検査だけで終わらせない。

```sh
python3 scripts/build_ffmpeg_lgpl.py --prefix target/native/ffmpeg-lgpl --verify-only
python3 scripts/package_macos.py \
  --prefix target/native/ffmpeg-lgpl --sources target/native/downloads \
  --output target/release-001/adhoc-package
python3 scripts/verify_package.py \
  --package target/release-001/adhoc-package \
  --relocated "$TMPDIR/kronello-release-001-adhoc-relocated" \
  --report target/release-001/adhoc-verification.json
```

手順3は清掃した環境で実 binary を起動する検証を含む。CLI の実 argv は `<relocated>/bin/kronello --request-json '{"operation":"capabilities.get"}'`、MCP は `<relocated>/bin/kronello-mcp` に `initialize`（protocolVersion `2025-11-25`）→ `notifications/initialized` → `tools/call`（`capabilities.get`, arguments `{}`）の newline JSON を渡す。両方とも exit 0、canonical `library_directory=<relocated>/lib`、`substituted=false`、loaded5本の固定 ABI / LGPL / configuration と必須 codec を要求する。`<relocated>/tools/release_roundtrip <report-stem>-artifacts/roundtrip` も exit 0 / `status=passed` を要求する。実際の argv / stdin に対する応答 / cwd / exit code は verification report の `commands` に保存される。続けて ad-hoc replacement の override / 同じ roundtrip と、欠落 override / swresample の期待 exit 1 を確認する。

手順3が成功した後、必要な GPU を持つ host で上記 `cargo test --workspace --locked` を実行する。公開配布は別候補に対する「Developer ID / notarization / Gatekeeper」の手動検証も必要。Windows / Linux package は別 platform 検証が必要。RELEASE-001 の完了判定、host の platform / revision / hashes / commands / exit codes 追記、backlog と ADR index の更新は supervisor が行う。

## supervisor のホスト検証（2026-10-05、Apple Silicon、macOS-27.0-arm64-arm-64bit-Mach-O）

統合ブランチを取り込んだ後の revision `a7a2b2a` で次を実行した。

| 手順 | コマンド | 結果 |
|---|---|---|
| LGPL prefix の確認 | `python3 scripts/build_ffmpeg_lgpl.py --prefix target/native/ffmpeg-lgpl --verify-only` | exit 0。5 library・AV1・ProRes・PCM24 を確認 |
| ad-hoc 署名済み package | `python3 scripts/package_macos.py --prefix target/native/ffmpeg-lgpl --sources target/native/downloads --output target/release-001/adhoc-package` | exit 0、package hash `cf9ca5658c8828990c890a65853c79ba0c8e5e55e20efc34dad729e29d2ca3bf` |
| 再配置・署名・実起動・差し替え・roundtrip（1 回目） | `python3 scripts/verify_package.py --package target/release-001/adhoc-package --relocated "$TMPDIR/…" --report target/release-001/adhoc-verification.json` | exit 1（`PACKAGE_VERIFICATION_ERROR: 2`） |
| 同（修正後） | 同じコマンド、report `adhoc-verification-2.json` | exit 0、`status=passed`、`acceptance_verified=true`、manifest `207e977a50f31297d99fed25de7a739bb76e558e951a7a462892d87eca9905ea` |

1 回目の失敗の原因: verifier の MCP 確認は initialize / initialized / `tools/call capabilities.get` を書いた直後に stdin を閉じていた。MCP-002 以降の stdio server は EOF で処理中の要求を協調停止するため（[08 API](../architecture/08-api-cli-mcp.md)）、`tools/call` の応答が返らなかった。`Runner.run_session` を追加し、全要求 id の応答を受け取ってから stdin を閉じるように直した。package 本体・runtime は変更していない。

公開用の Developer ID 署名・notarization・Gatekeeper の確認は Apple の資格情報が必要な手動工程であり、今回は実施していない（ad-hoc 署名での再配置後の起動までを確認した）。Windows / Linux の package は未検証で、保証経路に含めない。

