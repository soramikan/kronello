# fixture と解析的 golden scene

QA-001 の実装: fixture の生成・取得・台帳検証、`kronello-testkit` の意味的比較と CPU 画素比較、解析的期待値を持つ scene 定義を提供する。実際の編集モデル・組版・レンダーへの接続は後続タスクである。GPU の固定環境比較は [golden-comparison.md](golden-comparison.md) に分ける。

## 素材と権利

[ADR-0039](../adr/0039-test-fixtures.md) に従い、生成データは本プロジェクトの MIT OR Apache-2.0、取得フォントは OFL-1.1 とする。実写などの CC0 素材は現在未使用。出典・ライセンス・用途の台帳は [LEDGER.md](../../tests/fixtures/LEDGER.md)、機械可読の正本は [manifest.json](../../tests/fixtures/manifest.json)。

同梱の上限は **1 ファイル 256 KiB、素材合計 1 MiB**。同梱データは約 27 KiB。大きい素材は `target/fixtures/external/` に取得する。Noto Sans CJK JP Regular（Sans2.004、約 16 MiB）は upstream commit `523d033d6cb47f4a80c58a35753646f5c3608a78` と SHA-256、byte 数を固定し、OFL 原文を同梱する。結合濁点・IVS・emoji の入力があっても、この単一フォントに全 glyph があると保証しない。fallback と組版の期待値は TEXT-001 で追加する。

| 種類 | 内容 | 検証 |
|---|---|---|
| 日本語 JSON | 結合濁点、IVS、ZWJ emoji、異体字、禁則、ruby、縦書きの入力 | UTF-8 の byte hash、再生成一致、代表例の codepoint |
| 時刻 JSON | 24 / 25 / 30 / 30000/1001 / 60000/1001 fps、VFR、長尺フレーム | `{"num":"1","den":"24"}` の decimal string による正規化有理数。浮動小数点時刻は保存しない |
| straight alpha PAM | 透明有色、半透明、不透明、低 alpha | 固定 byte hash、再生成一致。内部画像へ取り込む際は変換が必要 |
| 線形 HDR RGBA16F | linear Rec.2020、premultiplied、little-endian binary16、4x1 | 負 RGB、1 超、alpha=0、微小 alpha の解析値。tone mapping を行わない |
| 48 kHz stereo WAV | PCM16、0.1 秒、固定整数表の 1 kHz sine、右 channel 切替 | 固定 byte hash、再生成一致、代表 sample 値 |
| CFR/VFR NUT | 16x16、rawvideo、6 frames | ffprobe の整数 PTS × 有理数 time_base、frame 数、rate、format、全 frame decode |
| PQ/HLG Matroska | 16x16、FFV1、10bit limited-range ramp、2 frames | BT.2020 / transfer / matrix / range と全 frame decode |

PQ/HLG ramp は符号値と metadata の入力 fixture であり、203 cd/m² の輝度校正、HDR 表示器、tone mapping の検証ではない。線形 HDR データの 1 は [ADR-0044](../adr/0044-color-and-alpha-contracts.md) の作業値を表す。

## 再現手順

Python 3.10 以上と FFmpeg / ffprobe が必要。使うのは rawvideo、FFV1、PCM と標準 filter だけで、GPL codec を必要としない。開発環境でインストールされた FFmpeg の本体・ライブラリは fixture に含めず、配布 FFmpeg の LGPL build 検証（MEDIA-001）とは分ける。

```sh
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py generate
python3 scripts/fixtures.py check --generated target/fixtures/generated
python3 -m unittest discover -s scripts/tests -v
cargo fmt -p kronello-testkit --check
cargo clippy -p kronello-testkit --all-targets --locked -- -D warnings
cargo test -p kronello-testkit --locked
```

`generate` は `target/fixtures/generated/data/` と `media/` に生成し、FFmpeg/ffprobe の版・configure、コマンド、生成 hash と probe 結果を `receipt.json` に保存する。同梱データは標準 Python だけで byte 一致に再生成する。FFmpeg container の byte hash は版間で同一とは約束せず、生成後の receipt hash と、rate・PTS・HDR metadata・decode という意味の契約を検証する。コンテナの全 byte 固定が必要な GPU scene では、この receipt と native tool の版も固定する。

`check` は同梱データと取得フォントの固定 hash、byte 数、サイズ上限、ID/台帳/scene 参照を確認する。`--generated` を付けた場合は、全生成物の receipt、PTS/metadata/decode、同梱データの再生成一致も必須とする。fixture の再生成だけで manifest の hash は更新しない。

`fetch_fixtures.py --offline` はネットワークを使わず、既存の取得素材を固定 hash で検証する。正常な cache は通常の取得でも再利用する。HTTPS のみを許可し、manifest の byte 数を超える取得を止め、一時ファイルを hash 検証してから確定名へ置換する。取得失敗、欠落、hash 不一致は非ゼロ終了し、テストを skip しない。フォントを更新するときは upstream commit、SHA-256、byte 数、OFL 原文、台帳と利用 scene を一緒にレビューする。

## Rust からの fixture 解決

`resolve_fixture(id)` は、この crate のリポジトリにある `tests/fixtures/manifest.json` を読み、fixture ID に対応する絶対・canonical path を返す。同梱素材はリポジトリ root を基準にした manifest の `tests/fixtures/` 配下、外部素材は既定で `target/fixtures/external/<filename>` を参照する。返す前にファイル全体を読み、byte 数と SHA-256 の両方を検証する。Rust はネットワーク取得を行わず、欠落時も失敗する。

外部素材ディレクトリは環境変数 `KRONELLO_FIXTURE_EXTERNAL_DIR` で上書きできる。この変数は **external ディレクトリそのもの**を指定し、Rust と Python の `fetch_fixtures.py` / `fixtures.py check` が共有する。相対パスの場合は実行時の working directory が基準となる。Python の明示的な `--output` / `--external` は環境変数より優先される。

```sh
export KRONELLO_FIXTURE_EXTERNAL_DIR=/tmp/kronello-fixtures
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py check
cargo test -p kronello-testkit --locked
```

任意の root を使う場合は `FixtureResolver::new(root)`、環境変数によらず外部ディレクトリを指定する場合は `FixtureResolver::with_external_dir(root, external)` を作り、`resolve(id)` を呼ぶ。これらは manifest を読み込んだ時点の一覧を保持するが、ファイルの整合性は `resolve` のたびに確認する。

`FixtureError` は `NotFound`（fixture ファイル欠落）、`SizeMismatch`、`HashMismatch`、`UnknownId`、`InvalidManifest`（manifest 欠落・JSON/版/metadata/安全な相対パス等の不正）を区別する。その他のファイル I/O は `Io`。固定 hash のない `generated` 素材や未知の storage は `UnsupportedStorage { id, storage }` を返す。generated receipt の Rust 解決はこの API の対象外である。

通常テスト `resolves_noto_sans_cjk_jp_from_actual_manifest` は実 manifest のフォントを解決し、未取得なら失敗する（`#[ignore]` は使わない）。一時ディレクトリのテストは型付きエラーと同梱素材を検証し、環境変数の unset/set は Rust の子プロセスと Python の CLI entrypoint の双方で確認する。

`timing.json` の `frame_times`、`long_frame_time`、VFR の `time_base`、scene の `descriptor.time` / `input.fraction` / `input.interval` / `input.times` は同じ `num` / `den` オブジェクトを使う。整数は符号付き十進文字列（先頭ゼロ・`+`・`-0` なし）、分母は正、分数は既約とする。整数 frame index と rate の文字列は既存の形式を維持する。Rust の `FrameDescriptor.time: [i64; 2]` は変更せず、scene を API に渡す呼出側が decimal string を整数へ変換する。

## 意味的比較と画素比較

`kronello-testkit` は backend を持たない独立した test 用 crate。`kronello-time` / `kronello-model` / wgpu / FFmpeg に依存しない。

- `compare_semantic(path, expected, actual)` は `PartialEq` による厳密比較。ID、bounds、変換、構造化 JSON などに使用する。差分には path と両値を含む。
- `compare_finite_values` は有限性を確認したうえで浮動小数点の値を厳密比較する。意味値に画素の誤差を適用しない。
- `compare_pixels` は CPU の `f32` RGBA 配列を比較する。入力型 `LinearFrame` は線形作業空間・premultiplied alpha を契約とする。RGBA16F の呼出側は binary16 を復号し、色変換や clamp を挟まない。

画素 API は解像度、region origin、正規化有理数時刻、色空間、color pipeline、sample 数、seed の一致を要求する。空画像、buffer 長不一致、NaN/Infinity、範囲外 alpha、alpha=0 の非ゼロ RGB は失敗する。負 RGB と 1 超、正の微小 alpha は保持する。全画素が許容誤差を満たすことを要求し、超過時は最大 RGB/alpha 誤差・超過画素数・最初の index を返す。環境 fingerprint の検証・画像保存は GPU harness の責務。

既定許容値は暫定版 1: RGB は `2^-10 * max(1, abs(expected))`、alpha は絶対誤差 `2^-10`。参照機での妥当性確認は未実施。閾値変更は scene の比較方式の版・理由と一緒にレビューする。

[scenes.json](../../tests/golden/scenes.json) は専用の test contract 形式であり、公開 `.kronello` schema や Command API ではない。線形補間、半開区間、変換 bounds、source-over、isolated Group opacity、HDR 非 clamp、unpremultiply の閾値、Unicode、PCM sample の 9 scene に入力と解析値を記録する。crate の通常テストで解析値と fixture の整合性を確認する。**実際の評価エンジン・組版・GPU が scene に適合したというテストではない**。後続の実装は同じ期待値を用いて actual 結果を比較する。

## CI と未実装

`.github/workflows/ci.yml` の macOS と Linux ジョブで、workspace test 前にフォント取得、fixture 生成・検証と Python の失敗経路テストを必須実行する。外部取得の失敗はジョブの失敗。通常の Rust テストには CPU の比較 API と解析値の整合性を含める。

未実装: GPU render adapter、参照機の fingerprint 採取、実測 RGBA16F/PNG の基準、画像差分 artifact、基準更新 harness、実測に基づく許容誤差の校正。`tests/golden/m4-macos-metal/` には未測定を示す README だけを置く。GPU-001 で参照機上にて実装・確認し、通常 CI の成功を Metal golden の成功と扱わない。
