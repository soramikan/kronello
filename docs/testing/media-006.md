# MEDIA-006 検証記録

実施日: 2026-10-10。ローカルファイルを共有 API で Asset に登録する経路。
既存の GUI / Lucide 関連の未コミット差分を保持し、コミットは作成していない。

## 実装と検証範囲

- `media.import` の strict request / registry / local locator 検証 / Event 応答。
  ファイル canonicalize、既存 SHA-256、FFmpeg のローカル demuxer と stream probe、
  A/V 各 stream の先頭フレームの decode、前後 hash 照合、Asset 検証を行う。
  静止画は既存 PNG decoder の色・animation 制限も検証する。
  RAW 自動 import、JPEG 等の未対応静止画、未対応色情報は型付き失敗。
- 共有 AssetSet の plan/apply、revision、receipt、Undo を利用する。
  後続編集・素材削除後の同一要求再送は元の Event、key の異なる利用は型付き拒否。
- CLI のフラグ / JSON、registry 由来 MCP tool、公開 JSON schema / Swift Codable を同期。
- Media / Edit Project の secondary 読み込みボタン、動画・音声・画像の複数選択と
  Finder file URL drop を共通 EditorModel に接続。1ファイル1 Event として順次 reload / Undo 登録する。
  型付きエラーで残りを停止。空状態文言と `file-input` の用途表を更新。

`crates/kronello-service/tests/media_import.rs` は native encoder で小さい ProRes・PCM24 を生成し、
PNG は既存 png crate で生成する。外部 ffprobe / shell / FFmpeg 引数は import に使用しない。
video / audio / image、A/V 混在の全 stream、codec・有理数時刻・サイズ・hash・locator、
revision conflict・receipt replay・Undo、欠落・非メディア・ディレクトリ・URL・未対応 PNG gamma を検証する。
CLI は実プロセスで flag → JSON 再送の同じ Event を検証。
MCP は discovery・schema・共有型付き失敗を検証する。
Swift は生成 Request の復号、ファイルごとの UUID / key・revision 更新・Undo・エラー UI 接続を検証する。

## コマンド結果

| コマンド | 結果 |
|---|---|
| `cargo fmt --all --check` | 成功 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 成功（全 workspace） |
| `cargo test -p kronello-service --locked --test media_import` | 3 passed |
| `cargo test -p kronello-service --locked --test media_import --test api` | API 12 passed、当該実行時の import 2 passed。A/V 混在を追加後、上行の import 3件を再実行して成功 |
| `cargo test -p kronello-cli --locked --test media_import` | 1 passed |
| `cargo test -p kronello-mcp --locked --test stdio media_import_is_discovered_and_dispatches_typed_errors` | 1 passed、17 filtered out |
| `cargo test -p kronello-service --locked` | 失敗。既存 `nle2::gpu_nle2_clip_effects_and_explicit_video_upload_match_cpu` が Metal adapter 不在。nle2 は22 passed / 1 failed。以降の test binary は未実行 |
| `cargo test --workspace --locked` | 失敗。既存 CLI `machine::gpu_headless_default_backend_animated_shape_japanese_text_sequence` が `ADAPTER_UNAVAILABLE`。machine は25 passed / 1 failed。以降は未実行 |
| `python3 scripts/generate_swift_api.py --check` | 成功 |
| `python3 scripts/gen_lucide_icons.py --check` | 成功 |
| `python3 scripts/build_ffi.py` | 成功。検証用のローカル FFI / CLI を再生成 |
| `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --package-path apps/macos -j 3` | 失敗。ホーム配下 Clang ModuleCache への書き込みが sandbox により拒否された |
| `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift test --package-path apps/macos -j 3` | 同じ ModuleCache 制約で失敗 |
| 下記のキャッシュ指定 + `swift build --package-path apps/macos -j 3 --disable-sandbox` | 成功 |
| キャッシュ指定のみ + `swift test --package-path apps/macos -j 3 --disable-sandbox` | AppModel は120件、1 skipped、38 failures。多くは既存 native テストの `JOB_STORAGE_ERROR`（保存先が書き込み不可）、1件は CancellationError。新規 MediaFlow の import テストは成功 |
| キャッシュ・state 指定 + `swift test --package-path apps/macos -j 3 --disable-sandbox` | AppModel は120件、117 passed / 1 skipped / 2 failed。SourceMonitor の `testInsertOverwriteThreePoint` / `testMulticamCreatePreviewAndSwitch` が `JOB_STORAGE_ERROR`。新規 import テストは成功 |
| `python3 scripts/backlog.py render` / `python3 scripts/backlog.py check` | 成功、144 tasks |
| `git diff --check` | 成功 |

Swift の再実行で使う環境変数（`HOME` は変更しない）:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
CLANG_MODULE_CACHE_PATH=/tmp/kronello-import-clang-cache \
SWIFTPM_MODULECACHE_OVERRIDE=/tmp/kronello-import-swift-cache \
KRONELLO_STATE_ROOT=/tmp/kronello-media-import-swift-state \
swift test --package-path apps/macos -j 3 --disable-sandbox
```

追加テストの初回実行では success envelope と空 assets の省略に対するテスト側の期待を修正し、上記の最終実行で成功を確認した。

state 指定の再実行では Design 19件・Core 4件も成功し、合計143件（140 passed / 1 skipped / 2 failed）。

skip は既存 `testStage2FFIPresentationParity`（`KRONELLO_INTEGRATION_EVIDENCE` 未指定）。
追加範囲は対象テスト・schema・ビルドで受け入れ、MEDIA-006 を done とした。
全体の GPU / integration 受け入れを成功扱いにはしない。
NSOpenPanel / Finder の実マウス操作はこの実行では未実施。確認手順は次のとおり。

1. プロジェクトの Media ページで「メディアを読み込む…」を押し、小さい MOV・音声・PNG を複数選択する。
2. 一覧の更新を確認し、Undo を1回ずつ実行して1ファイルずつ取り消す。
3. Media の空グリッドと Edit の Project リストへ同じファイルを Finder から drop し、同じ結果を確認する。
4. 非対応ファイルを drop して既存エラーダイアログを確認し、CLI `media query` で失敗した Asset が追加されていないことを確認する。

## 変更ファイル

- Rust: `kronello-service/src/{media,lib,api,wire}.rs`、`tests/{media_import,api}.rs`。
- native media: `kronello-media/native/media.c`、`src/{export,ffi}.rs`。
- CLI: `kronello-cli/Cargo.toml`、`src/{main,media_import}.rs`、`tests/media_import.rs`。
- MCP: `kronello-mcp/tests/stdio.rs`（tool 本体は既存 registry 自動公開）。
- Swift: `Kronello/MediaImport.swift`、`MediaPage.swift`、`EditPage.swift`、
  `KronelloAppModel/MediaImport.swift`、`KronelloCore/GeneratedAPI.swift`、
  `KronelloAppModelTests/MediaFlowChecks.swift`、`MediaFlowTests.swift`。
- 公開 schema、API / GUI 設計章、icons 用途表、backlog 正本・生成一覧、本記録。
