# GPU 画素の golden 比較（固定環境）

状態: GPU-001 で ignored `golden` harness を実装し、GPU-002 で coverage・group・mask・入出力色変換のシーンを加えて計 16 シーンとした。QA-001 の CPU 比較 API を使う。**M4 参照機の fingerprint・基準画像は未作成**。許容誤差は参照機での校正前の暫定値。M1 開発機では UPDATE による候補生成だけを検証し、基準への登録・比較合格とは扱わない。

[ADR-0038](../adr/0038-toolchain-and-ci.md) に従い、値・レイアウトの意味的比較は両 OS の通常の `cargo test --workspace --locked` に含める。GPU 画素の比較は固定環境で明示的に実行し、GitHub Actions の必須ジョブにはしない。golden test は `#[ignore]` とし、明示実行時の失敗や未実行を成功として扱わない。

## 対象環境と固定の単位

- 参照機: **M4 Mac mini / メモリ 32GB / macOS / Apple Silicon ネイティブ実行**。
- backend: **Metal**。ソフトウェア Vulkan、別 GPU、Rosetta での結果をこの基準と比較しない。
- Rust: `rust-toolchain.toml` の **1.95.0**、依存: `Cargo.lock`。OFL フォントと素材の版・SHA-256 も固定する（[ADR-0039](../adr/0039-test-fixtures.md)）。
- macOS の版と build、Metal driver、wgpu の版は、初回の GPU-001 実測時に採取した fingerprint を基準に固定する。未測定の OS 版を検証済みとして指定しない。
- 解像度、設計寸法、正規化した有理数時刻、作業用色空間、出力変換、alpha 表現、サンプル数・seed をシーンごとの manifest に記録する。壁時計や非固定乱数を入力にしない。

OS・driver・toolchain・wgpu 等を更新したら、環境更新としてレビューし、新しい fingerprint と基準画像を一緒に更新する。`macos-latest` は可変の CI 環境であり、この固定環境の代用にはしない。

## 保存場所

| パス | 内容 |
|---|---|
| `tests/golden/m4-macos-metal/environment.json` | 基準環境の fingerprint |
| `tests/golden/m4-macos-metal/manifest.json` | fixture / font の hash、シーン設定、比較方式・許容誤差の版 |
| `tests/golden/m4-macos-metal/<scene-id>/<frame-id>.rgba16f` | 色空間・alpha を manifest で明示した基準画像（little-endian RGBA binary16） |
| 同じ場所の `<frame-id>.png` | 人間向けの SDR 表示画像。HDR や alpha の数値比較の代用にはしない |
| `target/golden/run.*/` | 実測画像、差分、比較レポート、fingerprint、更新候補。Git 管理しない |

`frame-id` は manifest のサンプル ID とし、浮動小数点時刻の文字列から導出しない。初期は小さい生成シーンを基準として Git 管理する。QA-001 では同梱を 1 ファイル 256 KiB・合計 1 MiB 以下とし、大きい Noto フォントは upstream commit と SHA-256 を固定して取得する（[fixture 手順](fixtures.md)）。素材の出典・ライセンス・生成方法は台帳に記録し、外部 fixture は hash を固定する。取得失敗・基準画像欠落でテストを飛ばさない。

## 環境 fingerprint の採取

参照機でリポジトリのルートから実行する。以下の採取コマンドは OS / Cargo の既存機能を使う。GPU 比較コマンドは GPU-001 の harness を使う。

```sh
mkdir -p target/golden
export KRONELLO_GOLDEN_OUTPUT="$(mktemp -d "$PWD/target/golden/run.XXXXXX")"
sw_vers > "$KRONELLO_GOLDEN_OUTPUT/macos.txt"
uname -m > "$KRONELLO_GOLDEN_OUTPUT/architecture.txt"
sysctl hw.model machdep.cpu.brand_string hw.memsize > "$KRONELLO_GOLDEN_OUTPUT/hardware.txt"
system_profiler SPDisplaysDataType -json > "$KRONELLO_GOLDEN_OUTPUT/metal.json"
rustc --version --verbose > "$KRONELLO_GOLDEN_OUTPUT/rustc.txt"
cargo --version > "$KRONELLO_GOLDEN_OUTPUT/cargo.txt"
cargo metadata --locked --format-version 1 > "$KRONELLO_GOLDEN_OUTPUT/cargo-metadata.json"
shasum -a 256 Cargo.lock > "$KRONELLO_GOLDEN_OUTPUT/cargo-lock.sha256"
git rev-parse HEAD > "$KRONELLO_GOLDEN_OUTPUT/revision.txt"
git diff --binary HEAD > "$KRONELLO_GOLDEN_OUTPUT/working-tree.patch"
```

比較 harness はさらに、**実際に選択した** wgpu adapter の name / backend / device type / vendor / device / driver / driver info、要求した features / limits、描画・比較コードの版を `environment.json` とレポートに出す。FFmpeg を使うシーンでは、リンク先の版・build configuration も記録する。OS が driver 版を個別に返さない場合は、その事実と macOS build を記録する。

機種、メモリ、architecture、OS build、backend / adapter、Rust、wgpu・native dependency の版を基準環境と照合し、不一致なら画像比較前に失敗する。code revision、shader hash、fixture hash、`Cargo.lock` 全体の hash は provenance として基準・実測の両方に残す。描画コードや fixture の変更自体を環境不一致として拒否せず、意図した変更かを差分レビューする。未コミット・未追跡の入力を含む基準画像は採用しない。

## 実行コマンド

```sh
WGPU_BACKEND=metal KRONELLO_GOLDEN=1 \
  cargo test -p kronello-gpu --test golden --locked -- --ignored --nocapture
```

- `KRONELLO_GOLDEN=1`: 固定環境の画素比較を明示的に有効にする。通常の CI では設定しない。
- `WGPU_BACKEND=metal`: harness が Metal のみを選ぶ。backend を自動選択に戻さない。
- `KRONELLO_GOLDEN_OUTPUT`: 上の手順で設定した絶対パスへ実測・差分・レポートを出す。未指定なら明示実行を失敗させる。出力先はリポジトリの `target/golden/` 内の新規ディレクトリに限る。候補・実測・レポートの既存ファイルを上書きしない。
- `KRONELLO_GOLDEN_UPDATE=1`: 次節の更新候補生成だけで使う。通常の比較では未設定にする。

harness はテスト数・シーン数・フレーム数をレポートに記録し、対象ゼロ、adapter 不在、fingerprint 不一致、fixture / 基準画像欠落、非有限値で非ゼロ終了する。GPU が利用できない場合の黙示的 skip や CPU fallback は認めない。値・bounds・変換の意味的比較は ignored test に移さない。

## 許容誤差（提案・実測前）

[ADR-0044](../adr/0044-color-and-alpha-contracts.md) の作業用線形色・premultiplied alpha を保った RGBA16F を数値比較の正本とする。画像サイズ、色空間、alpha 表現、時刻・領域は厳密一致を要求する。

- RGB: 各画素・各成分で `abs(actual - expected) <= 2^-10 * max(1, abs(expected))`。
- alpha: 各画素で `abs(actual - expected) <= 2^-10`。有限値かつ `[0, 1]` は誤差とは別に検証し、alpha = 0 の内部 RGB は厳密にゼロを要求する。
- RGB の負値・1 超を clamp せず、HDR の差分を SDR の表示画像で代用しない。NaN / infinity は即失敗とする。
- SDR PNG を追加で比較するシーンは、同一の出力変換後、RGBA の各 8bit 成分の差を最大 1 に制限する。
- 全画素が条件を満たすことを要求する。超過画素の割合による許容、エッジの自動除外、位置ずらし、blur による差分隠しをしない。値・レイアウトの意味的比較には画素の許容誤差を適用しない。

QA-001 の `compare_pixels` はこの暫定値を既定値として実装済み。GPU-001 で同じシーンを同じ参照機で繰り返し測定し、暫定値が適切か確認する。許容誤差を変更する場合は manifest の比較方式の版と理由を更新し、回帰を通す目的だけで閾値を広げない。

## 基準画像の更新

1. 通常の意味的テストを通し、変更理由と対象シーンを明確にする。上の採取手順で新しい出力ディレクトリを作る。
2. 同じ参照機で更新候補を生成する。このモードの成功は画像比較の合格ではない。

   ```sh
   WGPU_BACKEND=metal KRONELLO_GOLDEN=1 KRONELLO_GOLDEN_UPDATE=1 \
     cargo test -p kronello-gpu --test golden --locked -- --ignored --nocapture
   ```

3. harness は `KRONELLO_GOLDEN_OUTPUT/candidate/` に fingerprint、manifest、画像を生成し、既存の基準を自動で上書きしない。UPDATE では基準画像の不在を許す。機種・backend・入力を検証・記録する。M1 等の非参照機でも経路確認用の候補を生成できるが、レポートで `eligible_reference_hardware=false` / `candidate_may_be_adopted=false` と記録し、M4 基準へ採用しない。通常比較は M4 Mac mini 32GB に限る。環境更新の候補には新旧 fingerprint の差分を添える。
4. 数値差分・PNG・alpha・HDR を確認し、意図した変更だけを `tests/golden/m4-macos-metal/` の対応ファイルへコピーする。出典・license、hash、比較方式の版も確認する。
5. `KRONELLO_GOLDEN_UPDATE` を未設定に戻して通常の比較を再実行し、テスト数とシーン数を確認する。基準・manifest・fingerprint と変更理由・実行結果を一緒にレビューする。

## 失敗時

1. 実測 RGBA16F、PNG、差分画像、最大誤差・超過画素数、レポート、fingerprint を `target/golden/run.*/` に保持する。基準画像を上書きしない。
2. 環境不一致なら macOS build / GPU / backend / Rust / 依存を照合し、基準環境へ戻すか、環境更新として別途基準をレビューする。別 GPU の失敗を許容誤差で吸収しない。
3. fixture / font の欠落や hash 不一致なら、台帳と取得手順を確認する。取得失敗は失敗のまま報告する。
4. 意味的テストと画素差分を照合する。意味的テストも失敗する場合はモデル・評価の回帰を先に修正する。画素だけなら色・alpha・shader・組版・driver の差を調査する。
5. 修正後、同じ固定環境で再実行する。golden の更新が必要な場合だけ上の更新手順へ進む。結果には実行環境と、合格・失敗・未実行を明記する。

## GPU-001 の実装範囲

GPU-001 の初期シーンは `source-over`、`srgb-pam`、`translated-rotation`、`rec2020-conversion`、`hdr-no-clamp`、`alpha-boundary` の 6 シーン・各 1 frame。GPU-002 の追加 10 シーンは次節を参照。RGBA16F は little-endian、線形作業空間・premultiplied alpha。HDR 作業値の保持は HDR 出力の保証ではない。QA-001 の `tests/golden/scenes.json` 全体を GPU 化したものではなく、時間・文字・音声等の解析的契約は通常の CPU テストが担当する。

環境情報と provenance を分けて保存する。UPDATE も同じ `compare_pixels` の既定許容誤差で CPU 参照画素を全画素検証する。比較モードは環境・manifest を照合した後、基準 RGBA16F を比較し、実測 RGBA16F、表示 PNG、符号付き差分 `difference.rgba32f`、差分表示 PNG、各シーンの最大誤差と超過画素数を保存する。表示 PNG は external unpremultiply → Rec.709 変換 → sRGB encode → clamp の閲覧用で、数値比較には使わない。

実測・候補に `environment.json` / `manifest.json` / `provenance.json`、実行先に `report.json`、失敗時に `failure.json` と失敗の `report.json` を出す。失敗レポートも test / scene / frame 数を記録し、frame 数は実際に保存できた RGBA16F の枚数とする。基準欠落・環境不一致は描画前に失敗するため、その場合は fingerprint / failure のみで画素差分はない。旧 fingerprint が存在すれば `environment-diff.json` も保存する。描画コード、shader、比較コード、fixture、lockfile の hash と Git revision / dirty 状態を記録する。未追跡入力の完全なアーカイブではないため、未コミット候補を基準として採用しない。

通常テストは [CI run 37072973888](https://github.com/soramikan/kronello/actions/runs/37072973888) の macOS / Linux (Mesa lavapipe) で成功した（revision `07a78ede6203575085b0a1a4a978a2d98877e8bd`）。FrameBridge の `tests/paths.rs` は両 OS で各 3 passed。Linux の Vulkan 転送経路・画素照合の成功を含むが、ignored の固定環境 golden 比較や M4 基準への登録・比較合格は含まない。検証範囲は [CI 記録](gpu-spike-m0.md#linux--macos-ci) を参照。

実行環境・実測結果・未確認事項は [M0 GPU スパイク報告](gpu-spike-m0.md) を参照。

## GPU-002 の追加シーンと AA 契約

`tests/golden/m4-macos-metal/scenes.json` はシーン定義のカタログであり、基準画像・fingerprint ではない。harness は従来 6 シーンに次の 10 シーンを追加し、計 16 シーン・16 comparison frames を扱う。

| シーン | 通常テストで照合する契約 |
|---|---|
| `isolated-nested-overlap` | 子が重なる 2 段 group。opacity を子へ配った誤結果と alpha / RGB が 0.1 超異なる |
| `alpha-matte-reference` | matte の alpha、端の coverage、入力参照として消費して重ね描きしない |
| `luma-matte-rec709` / `luma-matte-rec2020` | encoded sRGB を復号した線形 Y × alpha。作業原色ごとの係数 |
| `coverage-fill-stroke` / `coverage-srgb-rec2020` | 斜辺と基本 round stroke、coverage による premultiplied 色、両作業空間 |
| `fill-evenodd-hole` / `fill-nonzero-winding` | 同じ向きの内外 contour で Evenodd の穴と Nonzero の塗りを区別 |
| `glyph-outline-edges` | 固定 Noto Sans CJK JP の「あ」を layout → flatten（0.02 px）→ coverage raster。暗い縁を出さない |
| `srgb-output-roundtrip` | GPU の external straight sRGB 出力を共通の線形作業空間へ戻して全画素比較 |

coverage の定義は [05 章](../architecture/05-render-gpu.md#m1-gpu-002-の描画境界) の `gpu002-grid4-v1`。GPU / CPU とも固定 4×4 の pixel sample pattern、同じ winding / capsule 判定を使い、エッジを除外しない。CPU 参照は binary16 丸め前の値。許容誤差は QA-001 の既定値を維持する。サンプル配置は空間 AA であり、時間・motion blur の 16 サンプルを意味しない。manifest の `samples_per_frame` はこの初期 harness の空間サンプル数として記録する。

通常の `tests/scene.rs` は GPU 全画素を CPU 参照へ比較するほか、半画素 coverage・winding の穴・round cap・nested group の解析値、matte の非表示 / 明示表示・共有入力、形状 / 実 glyph の単色エッジ、zero alpha の厳密な RGB ゼロを検証する。外部変換は両作業空間・全 3 出力空間・両 alpha 表現・epsilon の下 / 一致 / 上を通常 GPU テストで確認する。`cpu_shader_parses_and_validates` は GPU 不在でも Naga の parser / validator を実行する。

manifest には全 draw-list（輪郭・paint・描画順・参照・opacity・mask mode）、AA 版、font hash、flatten tolerance を記録する。provenance は新 WGSL、GPU 実装、CPU 参照、シーン生成コードの hash も持つ。`srgb-output-roundtrip` の比較画像は線形 premultiplied RGBA16F、追加の `external-srgb-straight.rgba16f` は encoded straight の外部出力証拠で、内部値として扱わない。UPDATE も 16 シーンを CPU 参照に照合し、isolated group が誤結果と区別できなければ失敗する。

GPU 面の RGB 範囲超過は shader の sticky validation status と CPU 面境界検査で拒否し、後の描画で隠して続行しない。image readback に加え、4 bytes の status readback を TransferStats に記録する。

M4 の基準画像・fingerprint の作成と固定環境比較は未実施。M1 host での通常 GPU テストと UPDATE の候補生成は実装の意味・harness 経路の確認であり、M4 baseline の合格には数えない。


### GPU-002 の M1 host 検証

supervisor の Apple M1 MacBook Pro / Metal / macOS 27.0（build 26A428）で、作業開始時の HEAD `7575001f21637eb887fe1e63d448ef8b91644778` に GPU-002 の未コミット差分を適用した状態を検証した。基準登録用の revision ではない。

- `WGPU_BACKEND=metal cargo test -p kronello-gpu --locked -- --nocapture`: contracts **13 passed**、scene **15 passed**、固定環境 golden **1 ignored**。新規 scene suite の GPU / CPU 全画素比較、入出力変換、ネスト group、mask、glyph edge、overflow を含む。
- `WGPU_BACKEND=metal KRONELLO_GOLDEN=1 KRONELLO_GOLDEN_UPDATE=1 KRONELLO_GOLDEN_OUTPUT=<fresh target/golden/run.* directory> cargo test -p kronello-gpu --test golden --locked -- --ignored --nocapture`: **1 passed**、16 scenes / 16 frames、すべて CPU oracle validated。最終候補は `target/golden/run.gpu002.vQPJP7/`。`eligible_reference_hardware=false`、candidate-only、baseline comparison なし。
- 初回 host 実行では範囲外 RGB の拒否テストが失敗した。Metal の half 出力で有限値へ飽和しうるため、readback の非有限検査だけでは不足することを確認した。GPU の書き込み前 sticky status / CPU の面境界検査を追加し、後の不透明描画で隠れる overflow と外部 straight 出力だけの overflow の回帰も通した。

これらは supervisor が host で実行して返した結果。worker sandbox は Metal adapter 不在で、GPU 実行を確認していない。M4 の基準画像・固定環境比較・許容誤差校正、Windows / Linux での新しい scene pipeline 実行、性能測定は未実施。
