# GPU 画素の golden 比較（Apple Silicon + Metal）

VEC-004 は gradient の 8 シーン、FX-002 は version 2 の `fx002-rotation` / `fx002-nonuniform-rotation` / `fx002-shear-shadow` / `fx002-reflected-shear-shadow-rec2020` の 4 シーンを追加し、現在のカタログは **36 シーン・36 comparison frames**。新規 12 シーンの候補生成・採用・比較は統合ブランチで一度だけ行う（[VEC-004](vec-004.md)、[FX-002](fx-002.md)）。

FX-001 は blur / shadow の 3 シーンを追加し、FX-001 時点のカタログは **24 シーン・24 comparison frames**。24 シーンの採用・比較証拠は [FX-001](fx-001.md) に記録する。以下の QA-003 の 21 シーンは初回登録時点の履歴である。

QA-003 は [ADR-0047](../adr/0047-apple-silicon-metal-golden.md) に従い、GPU-001 / GPU-002 / VEC-003 の **21 シーン・21 comparison frames** を共通の基準へ比較する。初回の M1 基準登録・全シーン比較は成功した（[QA-003 の検証記録](qa-003.md)）。許容誤差は QA-001 の `compare_pixels` 既定値 `2^-10` を維持する。

値・レイアウトの意味的比較は通常テストで行う。GPU golden は `#[ignore]` とし、明示実行する。GitHub Actions の必須ジョブではない。UPDATE の成功は候補生成と CPU oracle 検証の成功であり、基準画像との比較結果ではない。

## 比較環境と provenance

- 対象は **Apple Silicon ネイティブ `aarch64-apple-darwin` + Metal**。harness はコンパイル対象の macOS / aarch64、`WGPU_BACKEND=metal`、実際に選択した adapter の backend を検査する。Rosetta、Intel Mac、Vulkan は拒否する。
- 一つの共有 baseline を使う。adapter 名、機種、メモリ、macOS の版 / build、driver、Rust / wgpu・native dependency の版は **provenance のみ**。基準環境との不一致で比較を拒否しない。`environment-diff.json` に差分を記録する。
- Rust は `rust-toolchain.toml` の 1.95.0、依存は `Cargo.lock`。OFL フォントと fixture の版・hash は [fixture 手順](fixtures.md) に従う。code revision、shader / renderer / CPU oracle / fixture / lockfile の hash と dirty 状態を基準・実測の両方に保存する。
- 解像度、設計寸法、正規化した有理数時刻、作業色空間、alpha 表現、サンプル数・seed、全 draw-list、stroke / gradient / coverage の意味版はシーン manifest で固定する。壁時計や非固定乱数を入力にしない。
- 性能計測の第一基準機は **M4 Mac mini 32GB** のまま（[13 章](../architecture/13-quality-performance.md)）。Vulkan / Windows の基準と世代間の許容誤差校正は QA-004 の範囲。

## 保存場所

| パス | 内容 |
|---|---|
| `tests/golden/apple-silicon-metal/scenes.json` | 36 シーンのカタログ、coverage / stroke / gradient / effect の意味版 |
| 同ディレクトリの `manifest.json` | 全シーン入力・設定、fixture / font hash、比較方式・許容誤差の版 |
| 同ディレクトリの `environment.json` / `provenance.json` | 基準生成時の環境と revision / コード・入力 hash。環境一致を要求しない |
| 同ディレクトリの `adoption.json` | 各採用ファイルの SHA-256 / byte 数、シーン設定、許容誤差、環境・provenance をまとめた採用 manifest |
| 同ディレクトリの `<scene-id>/frame-0.rgba16f` | little-endian RGBA binary16、作業用線形色・premultiplied alpha の数値基準 |
| 同ディレクトリの `<scene-id>/frame-0.png` | 人間向けの SDR 表示画像。数値比較の代用にはしない |
| `srgb-output-roundtrip/external-srgb-straight.rgba16f` | encoded straight の外部出力証拠。内部作業値とは区別する |
| `target/golden/run.*/` | candidate / actual、差分、report、environment / provenance、失敗情報。Git 管理しない |

同梱上限は **1 ファイル 256 KiB・合計 1 MiB**。採用スクリプトは全 candidate、保持する README / カタログ、既存 `tests/fixtures/` と他の `tests/golden/` ファイルを合わせて検証する。採用対象のシーンの実 byte 数は host 採用ログに記録する。大きい Noto フォントは外部 fixture とし、この同梱合計に含めない。

採用の CPU 回帰は `python3 scripts/test_golden_adopt.py` で実行する。dirty / revision / hash / platform / zero scenes / 非有限値 / サイズ上限の拒否と採用・publish 失敗時の復元を synthetic fixture で検証する。GPU の基準比較の代用にはしない。

## 初回生成・更新と採用

1. 通常テストを通し、変更理由をレビューする。コード・文書をコミットし、`git status --porcelain --untracked-files=all` が空であることを確認する。
2. リポジトリのルートで新規出力先を用意し、UPDATE 候補を生成する。

   ```sh
   mkdir -p target/golden
   export KRONELLO_GOLDEN_OUTPUT="$(mktemp -d "$PWD/target/golden/run.qa003.XXXXXX")"
   WGPU_BACKEND=metal KRONELLO_GOLDEN=1 KRONELLO_GOLDEN_UPDATE=1 \
     cargo test -p kronello-gpu --test golden --locked -- --ignored --nocapture
   ```

   harness が実際の adapter、`sw_vers`、`sysctl`、`system_profiler`、Rust / Cargo、依存版を採取する。UPDATE でも全画素を独立 CPU oracle と比較する。候補の `adoption.json` は全 artifact の hash を持つ。`report.json` の test / scene / frame 数は 1 / 36 / 36、`candidate_may_be_adopted=true` を確認する。
3. CPU oracle 結果、RGBA16F / 表示 PNG、alpha / HDR、環境・revision・hash、許容誤差の版をレビューし、明示採用する。

   ```sh
   python3 scripts/golden_adopt.py "$KRONELLO_GOLDEN_OUTPUT/candidate"
   ```

   採用スクリプトは working tree が clean、候補生成時も clean、候補 revision が現在の HEAD と一致、対象が Apple Silicon + Metal、成功 report と全シーン数・設定が一致、全ファイル hash / byte 数が一致、RGBA16F のサイズ・有限値・premultiplied alpha が有効であることを検証する。256 KiB / 1 MiB 上限も検証し、**変更前に採用 manifest と byte 集計を標準出力に出す**。検証後に staging directory から baseline 全体を置換する。失敗時は既存 baseline を保持する。UPDATE は baseline を直接書き換えない。
4. UPDATE を付けず、新規出力先で比較する。採用後の working tree には baseline 差分があるため dirty だが、通常比較は provenance として記録して実行する。

   ```sh
   export KRONELLO_GOLDEN_OUTPUT="$(mktemp -d "$PWD/target/golden/run.qa003.compare.XXXXXX")"
   WGPU_BACKEND=metal KRONELLO_GOLDEN=1 \
     cargo test -p kronello-gpu --test golden --locked -- --ignored --nocapture
   ```

   `KRONELLO_GOLDEN_UPDATE` がシェルで設定済みなら `unset KRONELLO_GOLDEN_UPDATE` を先に実行する。test / scene / frame 数 1 / 36 / 36、`status=pass`、全シーンの mismatch 0 を確認する。baseline と採用ログ・変更理由を一緒にレビューし、baseline をコミットする。

`KRONELLO_GOLDEN=1` と絶対パスの `KRONELLO_GOLDEN_OUTPUT` は必須。出力は root の `target/golden/` 配下の新規 directory に限る。既存の candidate / actual / report を上書きしない。adapter 不在、対象ゼロ、基準欠落、入力・比較方式の不一致、hash 不一致、非有限値は非ゼロ終了する。GPU 不在の skip や CPU fallback は行わない。

## 比較と許容誤差

基準のシーン manifest は現在の入力・設定と厳密一致を要求する。`adoption.json` の全ファイル hash を検証し、RGBA16F を読み、CPU oracle と baseline の **両方** へ全画素を比較する。provenance の revision は採用時の clean HEAD に限り照合し、通常比較時の revision / OS / 機種 / driver の一致は要求しない。

[ADR-0044](../adr/0044-color-and-alpha-contracts.md) の作業用線形色・premultiplied alpha を数値比較の正本とする。manifest の `comparison_version=1`、`rgb_absolute=rgb_relative=alpha_absolute=2^-10` を維持する。

- RGB: 各画素・各成分で `abs(actual - expected) <= 2^-10 * max(1, abs(expected))`。
- alpha: 各画素で `abs(actual - expected) <= 2^-10`。有限値かつ `[0, 1]`、alpha = 0 の RGB は厳密ゼロを別途検査する。
- RGB の負値・1 超を clamp しない。NaN / infinity は失敗。全画素が条件を満たす必要があり、超過画素の割合、エッジ除外、位置ずらし、blur を許容しない。
- PNG は external unpremultiply → Rec.709 変換 → sRGB encode → clamp の閲覧用。HDR / alpha 数値比較には使わない。

失敗時は `failure.json` / `report.json`、実測 RGBA16F、最大誤差・超過画素数、符号付き `difference.rgba32f` / 差分 PNG を保持する。基準を自動更新せず、意味的テストと画素差分、shader / 色・alpha / 組版・driver の provenance を照合する。許容誤差の変更は理由と比較版をレビューし、回帰を通すためだけに広げない。

## シーンと意味版

GPU-001 の 6 シーンは `source-over` / `srgb-pam` / `translated-rotation` / `rec2020-conversion` / `hdr-no-clamp` / `alpha-boundary`。

GPU-002 の 10 シーンは `isolated-nested-overlap` / `alpha-matte-reference` / `luma-matte-rec709` / `luma-matte-rec2020` / `coverage-fill-stroke` / `coverage-srgb-rec2020` / `fill-evenodd-hole` / `fill-nonzero-winding` / `glyph-outline-edges` / `srgb-output-roundtrip`。ネスト group、mask、両作業空間、winding、固定 Noto の「あ」、外部出力 roundtrip を含む。

VEC-003 の 5 シーンは `stroke-joins` / `stroke-caps` / `stroke-miter-limit` / `gradient-linear-fill-stroke` / `gradient-radial-fill-stroke`。

VEC-004 の 8 シーンは `gradient-repeat` / `gradient-reflect` / `gradient-focal-radial` / `gradient-conic` / `gradient-linear-straight` / `gradient-srgb-straight` / `gradient-srgb-premultiplied` / `gradient-text-fill`。各 gradient の spread / interpolation / interpolation_version / geometry / inverse transform と stop を manifest に記録する。

FX-001 の 3 シーンは `fx-gaussian-alpha` / `fx-shadow-srgb` / `fx-shadow-rec2020`。kernel は `fx001-separable-gaussian-transparent-rne16-v1`、effect id ごとの意味版は 1。全 sigma / offset / straight color / opacity と GPU shader・CPU effect / kernel code の hash を manifest / provenance に記録する。

FX-002 の 4 シーンは冒頭の通り。各 effect の manifest に実際の `kernel_version` / `semantic_version` と covariance / transformed offset を含める。追加 kernel は `fx002-affine-ellipse-lattice-rne16-v2`、意味版 2。top-level の legacy kernel / 意味版 1 の情報も保持する。

manifest schema は 3。coverage は `vec003-grid4-v2`、stroke は `vec003-centered-stroke-v1`、gradient は `vec004-explicit-interpolation-v1`。固定 4×4 の pixel sample pattern を GPU / CPU で共有する。`samples_per_frame=16` は空間 AA であり、時間・motion blur のサンプル数ではない。全 draw-list、font hash、flatten tolerance 0.02 px、gradient stops / paint transform / stroke join・cap・miter limit を記録する。

過去の UPDATE は旧方針の candidate-only として実行したもので、今回の baseline 登録へ流用しない。GPU-001 / GPU-002 の実測範囲は [M0 スパイク報告](gpu-spike-m0.md)、VEC-003 の実測は [検証記録](vec-003.md) を参照。新しい clean commit で生成した M1 候補を明示採用する。
