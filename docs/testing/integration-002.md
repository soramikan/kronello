# INTEGRATION-002 の検証

対象: `.worktrees/m3-integ2` / `m3-integ2`、基点 `e02292ae68040edfd3ad3d66f41f95a61f3726cc`。
2026-10-05、Darwin arm64 / Rust 1.95.0。GPU・SwiftPM・アプリの画面操作は worker sandbox では未実行。
実装完了と受け入れ合格を区別し、host の証拠を最後に追記する。

## 入力と driver

`scripts/demo_integration_m3.py` は stage-1 の `Demo` を継承し、同じ
`examples/integration-001.project.json` / `integration-001.definition.json` を読む。
stage-1 の driver は入力読み込みを `load_inputs()` に切り出しただけで、既定の実行内容は変更していない。
公開前に同じ一つの edition に `portrait` variant を追加し、base の Composition・公開入力・
padding [4,2]・intro 2/5秒・outro 3/10秒・中間 stretch を維持する。
variant の immutable authoring ID は UUIDv5 で別にし、型・既定値は edition 共通。
その後の作成・define・instantiate・retime・set_input・query・render は実 CLI / MCP の共有 API のみを使う。
SQLite を直接読まない。GUI 側に別の作品状態を作らない。

stage-1 の全検査（独立入力、帯追従、保護区間、shadow、固定 snapshot、期待する
`TEMPLATE_OVERFLOW` と出力未公開）を実行する。stage-1 が最後に意図的に残す overflow 入力を
検証済みの「日本語の字幕\n背景帯が追従」に戻してから、**同じ `.kronello`** に
同じ `definition_ref` / version / duration 8秒 / inputs の portrait instance を作る。
GUI で開くのはこの最終 project。stage-1 の固定 job は投入時 revision の成果物として別に保持する。

| variant | design_extent (`design_px`) | 出力 pixels | wrap / line_height | 行 / text layout高 | 帯 Position / Size |
|---|---|---|---|---|---|
| base / landscape | 320×180 | 3840×2160 | 250 / 16 | 2 / 32 | [28,118] / [258,36] |
| portrait | 180×320 | 1080×1920 | 48 / 16 | 4 / 64 | [28,218] / [56,68] |

解像度から variant を自動選択しない。portrait は明示 binding / max_lines=4、base は max_lines=2。
出力 pixels を組版幅へ混ぜない。stage-1 の古い検証文書の layout高20/36は現在の期待値として使わず、
現行の line_height × 行数（16/32/64）を明示して照合する。
portrait の自動折返しを、同じ proposed instance に明示した
「日本語の\n字幕\n背景帯が\n追従」の `template.preview` と比較する。
全 node の三段階 bounds と text-local layout_bounds が厳密一致し、両方の diagnostic が null であることを要求する。
未対応の line-detail API を追加したり、別の組版器で行分割を推測したりしない。

固定時刻は 0、2/5、1、77/10 秒。各 variant の expanded `scene.query` を
CLI / MCP の両方で取得し、全フィールド・配列順序・値を保持した canonical JSON bytes を比較する。
key を sort し、数値として同じ 1 / 1.0 の spelling だけを正規化する。
小数精度を落とさず、時刻の有理数文字列も変更しない。
帯寸法、text / band の期待する layout min / max、全内部 node の layout / ink / visual が frame 内であることを照合する。

`--backend gpu` が既定で、CPU は `--backend cpu-reference` の明示選択だけ。
`--resolution small` は stage-1 の CPU テスト用。`--render-variants` は追加の
4K landscape / 1080×1920 portrait image_sequence jobs（0 / 7秒、2 framesずつ）を実行し、
成功・frame数・時刻・backend・metadataの寸法・PNG IHDR寸法を確認する。
通常の24fps movie / 音声 mux / GPU golden の更新を含まない。

出力は新規 directory を要求する。`report.json` は全 request / check と失敗理由を保持し、失敗時は非0終了する。
`input.project.json` / `input.definition.json`、8組の `*.cli.json` / `*.mcp.json`、
`portrait-wrapping.json`、最終 `project.export.json`、`fonts.json`、`gui-evidence.json` を保存する。
大きい stage-1 画素は従来どおり report 内を要約し、frame artifact を別に保存する。
状態 root は指定した新規 path または output 内の state。実ユーザー状態は開かない。

## 自動検証と受け入れ条件

| 条件 | named evidence / test | 判定範囲 |
|---|---|---|
| 1. 同じ project の CLI / MCP 値・bounds | 8件の `*.canonical_parity` / `*.layout_size` / `*.layout_bounds` / `*.inside.*`、Python `IntegrationM3.test_small_cpu_reference_parity_and_portrait` | 実 process の共有 query、全 canonical bytes |
| 1. GUI の同じ project / revision / 表示値 | XCTest `IntegrationTests.testStage2FFIPresentationParity`、共有 `IntegrationChecks.verifyEvidence` | 実 `NativeProjectTransport` / `ProjectSession`、CLI dump の全 query フィールド、内部 stable ID、PropertyPresentation の丸め、三段階 bounds / normalized overlay / 寸法label、読み取り前後export不変 |
| 1. GUI query の頻度と失敗 | `IntegrationTests.testInspectionScheduling` / `IntegrationChecks.verifyScheduling` | 同じkeyのcache、150ms debounce、cancel、再生中0 request、pause後1 request、typed font/revision failure |
| 1. アプリ画面 | 下記 host 手順、gallery `TemplateInstanceInspection` / `TemplateInstanceBounds` × Dark/Light | 1440×900、Inspector実表示とViewer枠、focus、Metal（worker未実行） |
| 2. 同じ定義と再レイアウト | `variants.single_definition`、`portrait.preview.no_overflow`、`portrait.wrapped_equals_explicit_lines`、上記layout期待値 | 一つのedition、同じinputs / 尺、明示variant、四行の組版とframe内bounds |
| 両variantのGPU出力 | `--backend gpu --resolution 4k --render-variants` | stage-1 4K固定jobと追加4K横型 / 1080×1920縦型。host未実行 |

Python 実 binary テストは `KRONELLO_INTEGRATION_TESTS=1` の opt-in。通常は理由付き skip。
opt-in後の binary / font 欠落は失敗とし skip しない。
XCTest の evidence check は `KRONELLO_INTEGRATION_EVIDENCE` を必須とし、未指定は `XCTSkip`。
直接 Swift runner は同じ check を呼び、未指定時は明示 `SKIP`。skip をGUI parityの合格に数えない。

GUI の表示判断は [ADR-0077](../adr/0077-template-instance-read-only-inspection.md) に記録した。
内部値は読み取り専用、既存 `PropertyPresentation` の label / unit / multiplier と小数一桁、
vectorは単位なしの各成分、scalarは単位付き。Fillは既存と同じsRGB HEX。
Inspector / Viewer は同じ cache、同じ `InstancePath + NodeId` の選択を使う。
再生中は最後の結果と「再生中は停止時に更新」、停止して150ms idle後に更新する。
cancelしたFFI requestのRust実行中断を保証せず、Swift completionと古い結果の採用を中止する。
型付き失敗は `KRErrorLine`。内部矩形の操作 handle / editable field / keyframe navigator は出さない。
gallery の静的例は表示部品の検査用であり、実 FFI 値の証拠には使わない。

## worker の実行記録

shared `CARGO_HOME` / `CARGO_TARGET_DIR`、managed `TMPDIR`、`CARGO_BUILD_JOBS=3` を使用。
FFmpeg は `/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl` の pkgconfig / lib / bin。
未commit。backlog・ADR index・open questions・docs index・design-system 文書は変更していない。

以下はworkerが実行した結果。hostの測定値は含まない。

| command | exit / 結果 |
|---|---|
| `python3 scripts/fetch_fixtures.py` | 0、Noto SHA-256照合 |
| `python3 scripts/fixtures.py generate` | 0、9素材生成/decode |
| `cargo build -p kronello-cli -p kronello-mcp -p kronello-ffi --locked` | 0 |
| `python3 scripts/build_ffi.py` | 0、GUI link用dylib/worker配置 |
| `cargo fmt --all --check` | 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 |
| `cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_` | 101、初回153 passed / 1 failed / 2 filtered。既存MCP fixtureの20秒timeout |
| 同上、別state / `RUST_TEST_THREADS=1` の単独再実行 | 101、同じMCP fixtureの20秒timeout。stdio target全体41.15秒 |
| 同上、別state / `RUST_TEST_THREADS=1` / `--no-fail-fast`で全suiteを再検査 | 0、598 passed / 0 failed / 1 ignored / 11 filtered、87 target / doc-test結果。既存MCP fixtureも成功 |
| `python3 scripts/generate_swift_api.py --check` | 0、生成API未変更 |
| `KRONELLO_INTEGRATION_TESTS=1 python3 -m unittest scripts.tests.test_integration_m3 -v` | 0、2 passed、847.148秒、実driver131 checks / 8 canonical pairs |
| opt-inなしの同じPython test | 0、1 passed / 1 explicit skipped |
| `python3 scripts/check_gui_swift.py ... --run-checks`（隔離state / Swift6.4、evidence未指定） | 0、GUI7 / Motion14 / inspection scheduling成功。FFI evidenceは明示SKIP |
| 同上、evidence指定のfull runner | 1、既存checksとinspection scheduling成功後、固定caseの検査失敗 |
| 同上、`--skip-modules --run-checks --integration-only`、evidence指定の単独再検査 | 0、8 FFI cases、全CLIフィールド / Inspector丸め / 三段階bounds / Viewerラベル / export不変、schedulingも成功 |
| retained query dumpの独立した期待値再照合 | 0、text / bandのlayout min/max 16件と8 canonical pairs |
| `python3 -m py_compile scripts/demo_integration_m2.py scripts/demo_integration_m3.py scripts/check_gui_swift.py scripts/tests/test_integration_m3.py` / `git diff --check` | 0 |

初回 driver は stage-1 固定job成功後、新しい帯高の期待値40が実値36に合わず非0終了した。
期待値を line_height × 行数 + padding に修正し、初回を合格に数えない。
直接 Swift 初回は user-state SQLite がsandbox外で `JOB_STORAGE_ERROR`。
二回目は異なる Xcode compiler と既存moduleの Swift 6.3.3 / 6.4 不一致。
隔離した `KRONELLO_STATE_ROOT` と選択中の Xcode-beta の実compiler / SDKを明示して再検査する。
`swiftc` wrapper の xcrun cache書込み診断も、実compiler指定で回避する。

最終Swiftコマンドの環境と引数（直接compiler、SwiftPMではない）:

```sh
KRONELLO_STATE_ROOT="$PWD/target/integration-002-swift-state" \
KRONELLO_INTEGRATION_EVIDENCE="$PWD/target/integration-002-stage2-1/gui-evidence.json" \
python3 scripts/check_gui_swift.py \
  --swiftc /Applications/Xcode-beta.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
  --sdk /Applications/Xcode-beta.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
  --disable-plugin-sandbox --skip-modules --run-checks --integration-only
```

`target/integration-002-python-test.log` はfresh driverの全opt-in結果。
`target/integration-002-stage2-1/` は初回のstage-1 projectを公開APIで再利用したquery-onlyの再検査記録
（80 checks、project revision16、portraitを追加で配置）であり、fresh driverのrevision14と区別する。
FFIはその同じproject / CLI dumpを読んだ。追加したlayout min/maxの期待値はこのdumpから16件を独立再照合した。
`fonts.json` はretained manifestからも保存した。fresh driverに追加した同じ保存処理をhost再現に使う。
131 checksの後に追加したlayout min/max check（8件）とvariant renderのextent指定は、最終fileに存在する。
追加min/maxは上記再照合で確認し、GPU render branchは未実行。
`target/integration-002-swift-check.log` / `integration-002-swift-evidence.log` /
`integration-002-swift-evidence-retry.log` に失敗と成功を分けて保存した。
full FFI runnerの初回失敗は一般メッセージだったため、具体的な原因を確定していない。
caseと型付きfailureを直接報告するようtestを修正し、他の重いデモが終了した単独再検査は成功した。
この時間関係から負荷の影響は考えられるが、初回をtimeoutと断定しない。

workspace suiteの初回はMCP `public_api_fixture_commands_return_schema_valid_success_from_real_binary`
の20秒response timeoutでexit101（そこまで153 passed / 1 failed / 2 filtered）。
デモと同時実行だった。別state root、`RUST_TEST_THREADS=1`、他の重い検証なしでも同じtestが失敗した。
test内の同時編集 / worker threadの検査を無効化せず、Rust harnessのtest間並列だけを抑える。
supervisorの指示で既存MCP testのtimeoutは変更せず、`--no-fail-fast`で全suiteを再検査した。
この三回目はMCP fixtureを含む全targetが成功した（598 passed / 0 failed / 1 ignored / 11 filtered）。
先の二回のtimeoutを再現性のある合格へ読み替えず、実行ごとの結果を保持する。
成功時のcommandは以下。GPUを除外したCPU suiteであり、host GPU / SwiftPMの受け入れ証拠ではない。

```sh
CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=1 \
KRONELLO_STATE_ROOT="$PWD/target/integration-002-test-state-remaining" \
cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge \
  --locked --no-fail-fast -- --skip gpu_
```

三回のログは `target/integration-002-cargo-test.log` / `integration-002-cargo-test-serial.log` /
`integration-002-cargo-test-remaining.log`。supervisorはintegration branchのhost full-workspaceで
同じtestが通ると報告したが、その測定の具体的なrevision / platformは提供されておらず、
このworktreeのhost合格としては扱わない。今回の変更を統合したhost full-workspaceはsupervisorが実行する。

## host コマンド（順番に実行、worker未実行）

1. FFmpeg、fixture、release binary と FFI / UI font を準備する。

```sh
cd /Users/sora/Repositories/soramikan/kronello/.worktrees/m3-integ2
export CARGO_BUILD_JOBS=3
export PKG_CONFIG_PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib/pkgconfig
export KRONELLO_FFMPEG_LIB_DIR=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib
export PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/bin:$PATH
python3 scripts/fetch_fixtures.py --offline
python3 scripts/fetch_ui_fonts.py
cargo build -p kronello-cli -p kronello-mcp --release --locked
python3 scripts/build_ffi.py --release
```

2. Metal で stage-1 と両variantを同じ driver から生成する。output / state は新規path。

```sh
WGPU_BACKEND=metal python3 scripts/demo_integration_m3.py \
  --binary-dir "${CARGO_TARGET_DIR:-target}/release" \
  --backend gpu --resolution 4k --render-variants \
  --output-directory target/integration-002-host-metal
```

`report.json` の全check、`fixed-frames/sequence.json`、
`landscape-frames/sequence.json` / `portrait-frames/sequence.json` のbackendとPNG寸法を保存する。
stage-1固定jobの投入後編集と、GUIが開く最終revisionを混同しない。

3. 同じprojectのXCTestとSwiftPM buildを実行する（直接compilerチェックとは別の証拠）。

```sh
export KRONELLO_STATE_ROOT="$PWD/target/integration-002-host-metal/gui-state"
export KRONELLO_INTEGRATION_EVIDENCE="$PWD/target/integration-002-host-metal/gui-evidence.json"
swift test --package-path apps/macos --filter IntegrationTests
swift build --package-path apps/macos
swift run --package-path apps/macos KronelloDesignGallery "$PWD/target/integration-002-gallery"
python3 scripts/build_macos_app.py
```

4. 同じ `.kronello` と明示font locatorをアプリで開く。

```sh
KRONELLO_FONT_INPUTS="$PWD/target/integration-002-host-metal/fonts.json" \
  target/macos/Kronello.app/Contents/MacOS/Kronello \
  "$PWD/target/integration-002-host-metal/lower-third.kronello"
```

## host の画面 review（Dark / Light、1440×900）

1. Motionページを開き、base配置Composition（ID `89028f05-042b-49dc-a813-a45ab80c20d3`）を選ぶ。
   この生成fixtureでは左端のCompositionタブがbase配置A、右端がportrait配置。
   タブの表示名は既存GUIの「Composition」なので、対応するIDはmanifest / FFI testで照合する。
   Layersのtemplate配置を選択し、内部Layerのpopupでtext / bandを選ぶ。
   各名前は表示だけ、CLI `gui-evidence.json` のInstancePath / NodeIdで対応を確認する。
2. 同じ時刻0 / 2/5 / 1 / 77/10秒へseekする（24fpsではframe 0 / 9.6 / 24 / 184.8）。
   通常のframe seekで表せない2/5・77/10秒はCLI / FFI自動テストの有理数検査と区別し、
   手動は0 / 1秒を必須にする。必要ならUI-stateの有理数timeを閉じたアプリの表示状態fileに設定して再openする。
   値を作品へ丸めて保存しない。display timecodeは表示上のframe丸めである。
3. band Size `258.0 · 36.0`、Position `28.0 · 118.0`、Opacity `100.0 %`、Fill `#E6800D`、
   text Position `32.0 · 120.0`、font Size `12.0 px`、Wrap width `250.0 px` をCLI dumpと照合する。
   三段階のMin / Max / SizeとViewerの青い枠・整数寸法labelを同じstageで照合する。
   内部レイヤーには編集field・navigator・操作handleがない。
4. portrait配置Composition（`gui-evidence.json` のportrait caseのcomposition）へ切替え、同じinputsが四行になり、
   band Size `56.0 · 68.0`、Position `28.0 · 218.0`、Wrap width `48.0 px`、text layout `48 × 64`、
   shadowを含むvisual boundsがframe内にあることを見る。横型に戻って値とrevisionが不変であることを確認する。
5. 再生中は最後の内部値とink-mutedのnoteを保持する。pause後に更新し、素早いseekで古い結果が後から戻らない。
   revision / selection変更、font locator欠落の `FONT_MISSING` がKRErrorLineとして表示されることを確認する。
6. 1440×900のDark / Light両方でInspectorをscrollし、値の欠け・boundsラベルの重なり・配色・popupとsegmentedの
   keyboard focus / VoiceOverを確認する。青は選択bounds、amberは既存playhead、赤はfailureだけ。
   galleryの正常 / stale / typed failure / handleなしboundsの4枚を確認し、スクリーンショットとrevision / platformを保存する。

| host revision / platform | command / review | 結果 |
|---|---|---|
| 未実行 | Metal両variant・SwiftPM test/build・アプリ・gallery / 1440×900 Dark/Light | pending |

## 変更範囲と follow-up

新しいdriver / Python test、内部検査のAppModel / View、XCTest-backed checks、gallery sheetと
ADR-0077 / 本書を追加した。共有GUIのhookは `InspectorPanel.swift` / `MotionViewer.swift` のみ。
`EditorModel.swift` / `Transport.swift` / `EditorWindow.swift` / Editページは変更していない。
既存stage-1 driverの入力hook、direct Swift runnerの新check登録、gallery mainのsheet登録、
architecture10の検査説明を追加した。core API / schema / generated Swift / GPU baselineは未変更。
host証拠・統合後のGUI-003 / AUDIO-002との確認とbacklog更新はsupervisorが扱う。


## M3 統合再検査（2026-10-06）

統合後の SwiftPM / FFI / 三入口比較の結果と未確認範囲は
[M3 統合受け入れ](m3-acceptance.md) に記録した。過去の worker 検査と現在の実機検査を区別する。

### 統合 Metal 4K 出力と実 FFI 照合（2026-10-06）

主エージェントの第2段階 driver は `target/m3-acceptance/integration-metal/` に横型 / 縦型の
Metal 4K出力、font manifest、固定 snapshot、CLI / MCP の canonical pair、GUI検査用のevidenceを生成し、
147 checksすべてに成功した。共有 template 定義の再利用と縦型の改行 / boundsを確認した。
続けて統合 SwiftPM の `IntegrationTests.testStage2FFIPresentationParity` がその `gui-evidence.json` を読み、
実 FFI sessionの評価値と layout / ink / visual boundsを照合して成功した。

証拠: `target/m3-acceptance/integration-metal/report.json` / `gui-evidence.json` / `portrait-wrapping.json`、
`target/m3-acceptance/integration-metal.log`。実画面の内部レイヤー・boundsの主エージェント確認は追記する。
