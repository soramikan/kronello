# INTEGRATION-001 の検証

対象: `.worktrees/integration001` / `m2-integration-001`、基点 `66f1fb4497f2e6dcc512cf2727625ec491342377`。2026-10-04、Darwin arm64 / Rust 1.95.0、worker sandbox の明示 CPU reference。GPU は未実行。INTEGRATION-001 は host の 4K GPU 確認（下記）で `done` とした。

## 再現 driver と入力

`scripts/demo_integration_m2.py` は `kronello` / `kronello-mcp` の実プロセスと公開 Command / Query API だけを使う。内部 crate 呼び出し、Project / job SQLite への直接アクセスはない。`examples/integration-001.project.json` は authoring Composition と二つの空の配置先 Composition、`examples/integration-001.definition.json` は公開 headline / accent、intro 2/5 秒、outro 3/10 秒、中間 stretch、帯 padding [4,2]、最大2行を持つ。帯の drop shadow は sigma=0、offset=[6,6]、black alpha=0.5、opacity=0.6。

CLI が template を公開し、A（5秒・「日本語」・赤系）と B（6秒・「別の字幕」・青系）を別 instance として作る。同じ定義を複製しない。Sequence の一つの video track に、二つの配置先 Composition を `[0,8)` / `[8,14)` の CompositionClip として置く。A の 5→8 秒 retime と、二行の長い別文字・橙系への変更は公開操作で行う。

MCP は stdio の initialize / initialized / tools/list を交換し、各 query に明示 project path を渡す。path を省略した scene.query が `INVALID_REQUEST` になることも確認する。scene.query の `evaluation` と property.sample の `fonts` は [ADR-0053](../adr/0053-integration-evaluated-queries-and-render-tiles.md) の明示評価 mode。

`--backend` は gpu / cpu-reference、既定 gpu。暗黙 fallback はない。`--resolution` は 4k / small、既定4k。最終 image_sequence job はそれぞれ3840×2160 / 320×180。診断用の frame / 画素比較はどちらの mode でも320×180。14秒の要求を明示1/8 fpsで samplingし、0秒のAと8秒のBを2 frames出力する。24 fps の動画や ProRes / 音声 mux は今回の検証範囲ではない。

`--output-directory` は新規 directory を要求する。`--binary-dir` は既定 `${CARGO_TARGET_DIR:-target}/debug`。`--state-root` は指定した新規 directory、省略時は output 内の新規 `state`。driver / 自動テストは実ユーザー状態領域を開かない。全 request と response summary、CLI exit / stderr / elapsed、check の結果は `report.json`、MCP の診断は `mcp.stderr.log`。失敗時も report を残し、非0終了する。大きい画素配列は count / SHA-256 summary とし、確定 numeric / PNG artifact は保存する。

## 自動テストの opt-in と CI

`scripts/tests/test_integration_m2.py` の実 binary テストは `KRONELLO_INTEGRATION_TESTS=1` を明示した場合だけ実行する。通常の `python3 -m unittest discover -s scripts/tests -v` では small / 4K の2件を理由付き `skipped` として報告する。4K は追加で `KRONELLO_INTEGRATION_4K=1` が必要。opt-in 後の binary / font 欠落や driver の失敗はテスト失敗とし、自動 skip しない。

macOS / Linux の CI は既存の FFmpeg と固定フォントの準備を使い、`Run workspace tests` の後に CLI / MCP の debug binary を build して small の1件だけを実行する。このテストは driver に `--backend cpu-reference --resolution small` を明示する。4K CPU / GPU は CI の対象外で、host で明示実行する。

```sh
python3 scripts/fetch_fixtures.py
cargo build -p kronello-cli -p kronello-mcp --locked
KRONELLO_INTEGRATION_TESTS=1 \
  python3 -m unittest scripts.tests.test_integration_m2.IntegrationM2.test_small_cpu_reference -v
```

4K CPU の host 専用コマンド（release binary と FFmpeg / 固定フォントを準備する）:

```sh
cargo build -p kronello-cli -p kronello-mcp --release --locked
KRONELLO_INTEGRATION_TESTS=1 KRONELLO_INTEGRATION_4K=1 \
  python3 -m unittest scripts.tests.test_integration_m2.IntegrationM2.test_4k_cpu_reference -v
```

`.gitattributes` は全 text を checkout 時も LF に固定する。これにより storage の `committed_schema_matches_rust_types`、service の `public_schemas_match_rust_generators`（両公開 schema）、`scripts/fixtures.py` の固定 JSON fixture の hash / canonical bytes 比較を Windows の CRLF 変換から保護する。PNG / RGBA16F / PAM / WAV などは binary とし、バイトを変換しない。

## 受け入れ条件との対応

| 条件 | driver の check / 回帰 | 根拠 |
|---|---|---|
| 1. CLI作成 / MCP確認 | `mcp.initialize`、`mcp.tools.list`、`mcp.scene.query:INVALID_REQUEST`、`instance.*.text/color/duration/shadow`、`capabilities.shadow` | 実CLIの作成・編集、実MCPのscene/query/sample。30 tools。暗黙作品なし |
| 1. 固定snapshotから4K出力 | `job.fixed_snapshot`、`job.post_submit_edit`、`job.frame.*.metadata/png/rgba16f/hash`、`job.frame.*.frozen_pixel_probe`、`job.ffprobe_codec` | MCP submit直後にCLIで文字を変更。job.getの投入revision/hash、frame metadata、0/8秒、2 frame、3840×2160。4Kの帯内画素も投入前の値と比較。**GPU行はhostで確認** |
| 2. 別instanceの文字 / 色 / 長さの非干渉 | `independence.evaluated_values`、`independence.pixels` | Aの3入力変更の前後でBのtext/layout/Property/effects/TimeMapと全linear画素が厳密一致 |
| 3. 5→8秒、別文字、背景帯 | `instance.*.band_follows_text`、`band.longer_text`、`one_directional.authoring` | text-local layout_boundsをworldへ写し、幅 / 高さに2×paddingを加えたsize、左上positionとsample値を照合。元のtextとauthoring Compositionは不変 |
| 3. 保護区間 | `protected_intervals.before/after` | 全有理数control pointsを比較。intro2/5秒・outro3/10秒を保持し、中間43/10→73/10秒 |
| 3. overflow検出 | `overflow.typed_error`、`overflow.no_published_output` | 長文でmax_lines超過。実workerのjob.getがfailed / `TEMPLATE_OVERFLOW`、確定output directoryなし |
| 3. 基本shadow | `shadow.5_seconds`、`shadow.8_seconds` | source外・offset後のband内部でalpha=1×0.5×0.6、RGB=0。offset元はalpha=1、shadow範囲外は全成分0 |
| 共有query / FX registry | service `integration_query` 2件 | shadow付きtemplate.define成功、text置換、band値、sample一致、query後export一致。既定GPUのServiceでもqueryはGPU不要。font欠落 / 重複 / URI / hash、inactive終端を型付き拒否 |
| 4Kの中間面抑制 | render `tiled_frame_matches_full_frame_across_shadow_halo_and_partial_tiles` | shadow + blur、0.5 / 1 / 2倍、512px境界と部分tile。単一DAGのCPU全linear / display画素と厳密一致、metadataは元region |

通常のlayout_bounds幅はwrap_width（250 design_px）であり、帯幅は258。二行文字で高さは20→36へ増える。tight ink boundsへ仕様変更していない。PNG寸法は全frameのIHDR / metadata、frame数はmanifest / job.get / 有理数格子、codecはPNG magicとLGPL ffprobeのpng stream / 1 packet / byte数で検証する。このLGPL構成のffprobeはPNGのwidth / heightを0と返したため、ffprobeを寸法検証の根拠にはしていない。ProRes検証への自動切替はない。

## ローカル環境とコマンド

shared Cargo cache / target とmanaged scratchを使用した。workspace-wide cargo、GPU probe、commit / push、golden更新は実行していない。

```sh
export CARGO_HOME=/Users/sora/.local/share/codex-bridge/cache/cargo
export CARGO_TARGET_DIR=/Users/sora/.local/share/codex-bridge/cache/targets/627b6e4b0e9dd70938a94524ccf4ec36c464091c940c40307f13bdef84500896
export TMPDIR=/Users/sora/.local/share/codex-bridge/scratch/worker_a4c509b43d9044519b542cf0112552d4
export PKG_CONFIG_PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib/pkgconfig
export KRONELLO_FFMPEG_LIB_DIR=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib
export PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/bin:$PATH
```

<!-- COMMAND_RESULTS -->

| command | exit | 結果 |
|---|---:|---|
| `cargo build -p kronello-cli -p kronello-mcp --locked`（初回、query追加後、registry修正後、tile追加後） | 0（各回） | dev binaries |
| `python3 scripts/fixtures.py generate` | 0 | 9素材生成 / decode |
| `python3 scripts/fetch_fixtures.py --offline` | 1 | Noto fixture欠落を検出 |
| `python3 scripts/fetch_fixtures.py` | 0 | Noto SHA-256照合 |
| `python3 scripts/demo_integration_m2.py --backend cpu-reference --resolution small --output-directory target/integration-001-cpu-small-1` | 1 | effect descriptor未登録によるtemplate.defineの`INVALID_EDIT`。修正後query回帰で検証 |
| 同上、output `target/integration-001-cpu-small-2` | 1 | driverのMCP成功value decodeを修正 |
| 同上、output `target/integration-001-cpu-small-3` | 1 | driverのlinear画素配列の扱いを修正 |
| 同上、output `target/integration-001-cpu-small-4` | 1 | 固定wrap幅の契約に従い、二行入力による高さ増加へcheck修正 |
| 同上、output `target/integration-001-cpu-small-5` | 1 | 固定job・全数値画素一致は成功。PNG ffprobe寸法0を確認し、寸法はIHDR、codecはffprobeとして検証項目を明示 |
| `KRONELLO_SCHEMA_UPDATE=1 cargo test -p kronello-service --test nle_schema --locked` | 0 | 1 passed。schema生成時の実行 |
| `cargo test -p kronello-service --test integration_query --locked` | 0 | 2 passed |
| `cargo test -p kronello-render --test render --locked tiled_frame_matches_full_frame_across_shadow_halo_and_partial_tiles -- --exact` | 0 | 1 passed / 34 filtered |
| `cargo test -p kronello-service --test api --test template --test nle --test editing --locked` | 0 | API11 / template5 / NLE14 / editing16、計46 passed |
| `cargo test -p kronello-cli --test machine --test jobs --locked -- --skip gpu_headless_default_backend_animated_shape_japanese_text_sequence` | 0 | machine16 / jobs13、計29 passed / GPU1 filtered |
| `cargo test -p kronello-mcp --test stdio --locked` | 0 | 10 passed |
| `python3 -m unittest discover -s scripts/tests -p test_integration_m2.py -v`（smallのみの初回版） | 0 | 1 passed、220.533秒（dev、並行compile / renderあり） |
| `cargo test -p kronello-render --test render --test template --locked -- --skip gpu_` | 0 | render31 / template5、計36 passed / GPU4 filtered |
| `cargo test -p kronello-model --test effects --locked` | 0 | 3 passed |
| `cargo build -p kronello-cli -p kronello-mcp --release --locked` | 0 | 最適化binary、100秒 |
| `cargo clippy -p kronello-model -p kronello-render -p kronello-service -p kronello-cli -p kronello-mcp --all-targets --locked -- -D warnings` | 0 | 警告なし |
| `cargo test -p kronello-service --test nle_schema --locked` | 0 | 1 passed、更新flagなしでproject / API schema生成一致 |
| `cargo fmt --all --check`、`python3 -m py_compile scripts/demo_integration_m2.py scripts/tests/test_integration_m2.py`、`python3 scripts/backlog.py check`、`git diff --check` | 0（各回） | backlog60 tasks |
| `cargo fmt --all`（実装追加後2回） | 0（各回） | Rust整形 |
| `python3 scripts/backlog.py render`（着手時のno-op、status変更後の再生成） | 0（各回） | INTEGRATION-001のin_progressを確認 |
| `python3 scripts/demo_integration_m2.py --binary-dir "$CARGO_TARGET_DIR/release" --backend cpu-reference --resolution 4k --output-directory target/integration-001-cpu-4k-1` | 0 | **55 checks / 2 frames**。fixed job 338.729秒、revision10、編集後revision11 |
| `KRONELLO_INTEGRATION_BINARY_DIR="$CARGO_TARGET_DIR/release" python3 -m unittest discover -s scripts/tests -p test_integration_m2.py -v` | 0 | **2 passed**、355.543秒。4K job338.638秒 / small job1.483秒、各2 frames |

4K CPU の report は `target/integration-001-cpu-4k-1/report.json`（699 request / response summary）、成果物は同 directory の `fixed-frames/`。各 PNG / RGBA16F / JSON / manifest の hash、3840×2160、2 frames、0 / 8秒、cpu_reference_float32、固定revision10、A/B帯の投入前画素を検証した。初期driver失敗を成功回数に含めない。Rust integrationの最終suite集計は **127 passed / 0 failed / 0 ignored / GPU5 filtered**（targeted tile再実行とschema生成時の重複は加算しない）。job.listの進捗確認も実CLIの公開APIから行い、exit0、running 0/2→succeeded 2/2を確認した。SQLiteは直接読んでいない。

準備時のPython here-documentはzshのtemporary file権限で2回実行前に失敗した（後続render / buildを含むcompound command全体のexitは0）。managed TMPDIRの設定と、temporary here-documentを使わないPython `-c`で必要な編集を実施し、現在のファイル・backlogのstatusを再確認した。Rust compile / test失敗ではない。

`scripts/tests/test_integration_m2.py` は同じdriverのsmallと4Kを実binaryで実行する。smallの既定はdev、4Kは所要時間を抑えるrelease。`KRONELLO_INTEGRATION_BINARY_DIR`を指定すると両方を同じdirectoryへ向けられる。実ユーザー状態は使わず、各testが別のTemporaryDirectory内のstate rootを明示する。最終2件版は両方releaseで **2 passed / 0 failed**。test stdoutのchecks=45は異なるcheck名の数で、driver reportは入力変更前後の同名checkを含む55回の検査を記録する。成功した4K / smallの各jobと、期待するoverflow failed jobを区別して確認した。

今回追加したコードは、FX descriptorの編集registry登録とcapabilitiesの実装effect列挙、既存compilerを使う明示評価query、512×512の大解像度tile実行。query回帰2件とtile回帰1件、公開API schema、ADR-0053とarchitecture05 / 08、driver / Python binary tests / input例を追加・更新した。`schemas/project-v1.schema.json`、既存template / NLE例、GPU goldenは変更不要であり未変更。NLEの古い将来A/V muxコメントは今回のimage_sequence範囲へ訂正した。

```sh
cargo build -p kronello-cli -p kronello-mcp --release --locked
KRONELLO_INTEGRATION_TESTS=1 KRONELLO_INTEGRATION_4K=1 \
  KRONELLO_INTEGRATION_BINARY_DIR="${CARGO_TARGET_DIR:-target}/release" \
  python3 -m unittest discover -s scripts/tests -p test_integration_m2.py -v
```

## Supervisor の 4K GPU 実機コマンド

以下は **workerで未実行**。同じworktreeの変更を統合し、Metal adapterがあるhostで実行する。output / state pathは新規を使う。

```sh
cd /Users/sora/Repositories/soramikan/kronello/.worktrees/integration001
export PKG_CONFIG_PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib/pkgconfig
export KRONELLO_FFMPEG_LIB_DIR=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/lib
export PATH=/Users/sora/Repositories/soramikan/kronello/target/native/ffmpeg-lgpl/bin:$PATH
python3 scripts/fetch_fixtures.py --offline
cargo build -p kronello-cli -p kronello-mcp --release --locked
WGPU_BACKEND=metal python3 scripts/demo_integration_m2.py \
  --binary-dir "${CARGO_TARGET_DIR:-target}/release" \
  --backend gpu --resolution 4k \
  --output-directory target/integration-001-gpu-4k \
  --state-root target/integration-001-gpu-state
```

| revision / platform | command | exit | frame / check数 | 結果 |
|---|---|---|---|---|
| `31ea36f`（clean commit）/ Apple M1・macOS・Metal（`WGPU_BACKEND=metal`）、release build、2026-10-04 | 上記4K GPU driver | 0 | 3840×2160 PNG / RGBA16F 2 frames（0秒・8秒）、55 checks、130 request summaries | `status=verified`、全 check 合格。job `f684fcbd-184e-4e77-8b0e-8b38cf1dc821` は `succeeded`（投入から完了まで 48.1 秒、driver 全体 56.7 秒）。frame metadata の backend は `wgpu_rgba16f`。`sips` で両 PNG が 3840×2160 であることを確認。同梱 LGPL ffprobe は PNG の寸法を 0×0 と報告するため寸法の根拠にしない（codec=png は一致） |

host成功後にsupervisorがこの行を記入し、backlogをdoneへ更新する。CPU結果、GPU故障注入テスト、completed worker turnを4K GPUの受け入れ合格に数えない。固定GPU golden baselineは変更していない。

## macOS の一時 directory パス比較修正

2026-10-04、基点 `31ea36fa3c3165eb7e3ceeb244f44b02e5309fb5`。`scripts/tests/test_integration_m2.py` の `state_root` 期待値を `str(state.resolve())` に変更し、macOS の `/var` と `/private/var` の symlink 表記差を正規化して比較する。driver は既に状態パスを `resolve()` して使用・記録しており、`scripts/demo_integration_m2.py` は変更不要で未変更。

worker sandbox で、上記の shared Cargo cache / target と FFmpeg の3環境変数を使用し、今回の `TMPDIR` は `/Users/sora/.local/share/codex-bridge/scratch/worker_cd81b8c5127b4262afc81b98b87ffe64` に設定した。4K / GPU は今回未実行で、supervisor の host 検証に委ねる。

| command | exit | 結果 |
|---|---:|---|
| `cargo build -p kronello-cli -p kronello-mcp --locked` | 0 | debug binaries |
| `python3 -m unittest scripts.tests.test_integration_m2.IntegrationM2.test_small_cpu_reference` | 0 | 1 passed、145.762秒、45種類のcheck、2 frames。job 31.504秒 |
| `git diff --check` | 0 | whitespace errorなし |

## Supervisor の host 検証（2026-10-04）

- 4K GPU driver: 上表のとおり exit 0、55 checks。
- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets --locked -- -D warnings` / `cargo test --workspace --locked --no-fail-fast`: すべて exit 0、471 passed（一時 `KRONELLO_STATE_ROOT`）。
- GPU golden（`KRONELLO_GOLDEN=1`、Metal）: 24 scenes すべて pass、mismatch 0。tile 実行の追加後も baseline は不変。
- `python3 -m unittest discover -s scripts/tests`: 20 tests OK（4K / small の CPU reference driver を含む。state_root 比較の修正後）。
