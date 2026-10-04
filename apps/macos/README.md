# macOS FFI package

FFI-001 の検証用 SwiftPM package。実アプリの画面・デザイントークンは含まない。
package 名は `Kronello`、Swift tools 5.10、macOS 14 以上。
後続の `KronelloDesign` / `Kronello` targets はこの変更では作成しない。

| target | 内容 |
|---|---|
| CKronelloFFI | C header / module map。Rust の9関数だけを公開 |
| KronelloCore | MainActor の async wrapper と公開 schema 由来の Codable 型 |
| KronelloPreviewHarness | 一つの AppKit window と CAMetalLayer。編集・プレビューの検証専用 |
| KronelloJSONBenchmark | JSON encode / decode の単体計測 |
| KronelloCoreTests | Swift/CLI Event 同等性、生成型・raw transport の検証 |

## build / test

repository root で実行する。Rust 1.95.0、Swift compiler / macOS SDK、既存 native dependencies が必要。
共有環境では設定済みの `CARGO_HOME` / `CARGO_TARGET_DIR` / `TMPDIR` を維持する。

```sh
python3 scripts/generate_swift_api.py --check
python3 scripts/build_ffi.py
swift build --package-path apps/macos -j 3
swift test --package-path apps/macos -j 3
swift run --package-path apps/macos --skip-build KronelloJSONBenchmark examples/ffi-json-benchmark.request.json
```

`build_ffi.py` は `cargo build -p kronello-ffi -p kronello-cli --locked` を jobs=3 で実行し、
Cargo の artifact JSON から cdylib / CLI の実際の出力先を取得する。
`Libraries/libkronello_ffi.dylib` と `Libraries/kronello` はローカル生成物で Git 管理しない。
C header は手書きで Rust の全署名との一致を `header_matches_every_exported_function_signature` が検査する。
Swift ファイルを更新する場合は `python3 scripts/generate_swift_api.py` を使う。

Rust library の install name は `@rpath/libkronello_ffi.dylib`。
SwiftPM が絶対パスのローカル `Libraries` を link / rpath に加える。
配布用 app bundle、code signing、notarization、同梱 LGPL FFmpeg runtime の組み立ては後続範囲。
FFI build は FFmpeg executable や GPL binary を同梱しない。

## API / ownership

`ProjectSession(path:workerExecutable:)` はすぐに handle を返す。ファイルの検査結果は
`await ready()` で共有 `project.info` Response として取得する。
`call(API.Request)` は共有 Request / Response を使い、毎回明示的な project path を持つ。
`rawCall(Data)` は共有 strict decoder に生 JSON を渡す。重複 key の拒否も CLI と同じ。
`render.submit` には同じ revision の CLI worker executable を open 時に指定する。
指定がなければ job を作る前に `WORKER_EXECUTABLE_REQUIRED` を返す。

C 側の入力は関数内でコピーし、Rust worker に借用を残さない。入力上限16 MiB。
poll は `response_json` に共有 Response の JSON テキストを入れる。
Swift はこの文字列を Data として保持し、raw 応答中の未知の巨大整数を丸めない。
生成型の `JSONValue.number` は Foundation Decimal を使うため、
Decimal の精度を超える未知 JSON 数値を保持・再送する操作には rawCall を使う。
通常の Event UInt64 revision と Rational の10進文字列は生成型でも正確に保持する。
Project の未知 field と decode 時の明示的 null は生成型の再 encode で保持する。
schema の数値範囲・文字列 pattern・配列長・RenderInput の排他的 target 等の検証は共有 Rust API が正本。

要求は一つの専用 Rust thread で FIFO 実行する。最大64件の未回収 completion。
キューが満杯なら enqueue は0を返し、Swift は `NativeError.rejected` とする。
revision / job notification は250 msの idle interval と要求完了後に検査し、最新 snapshot に集約する。
`subscribe()` と `onNotification` を使い、idle 時は AppKit timer 等から `poll()` を呼ぶ。
Swift wrapper は待機中にも poll し、30秒で timeout する。
Task の cancel / timeout は待機を終了するが、既に受け付けた編集は取り消さない。
`close()` は新規要求を止める。受け付け済み work は worker で終了して資源を解放する。

## Metal preview の実機手順

この手順は Metal を使える Apple Silicon host で行う。
新しい project path を選ぶ。harness は fixture を create し、Shape size を `24 x 16` に edit.plan / edit.apply して Event を stdout に出す。

```sh
mkdir -p target/ffi-host
swift run --package-path apps/macos --skip-build KronelloPreviewHarness \
  target/ffi-host/preview.kronello examples/ffi-preview.project.json apps/macos/Libraries/kronello
```

一つの window に黒背景と赤い Shape が見えること、window resize 後も表示が更新されることを確認する。
stdout の `Presented` に `backend: metal`、編集後 revision、`image_readbacks: 0` が出る。
CAMetalLayer は attach 時に Rust が retain し、surface より後に release する。
NSView への取り付けと drawableSize 更新は AppKit main thread、
adapter / DAG / GPU / surface 操作は Rust worker に分離する。
画素は CPU / JSON を通らず、GPU RGBA16F texture から surface に描く。
4-byte shader validation status の readback は既存 GPU pipeline と同じ。
表示は linear premultiplied Rec.709 を黒に合成して sRGB SDR encode / clip する。
HDR tone mapping は実装しない。最終書き出しの色契約は変えない。

記録する証跡は window の screenshot、resize 前後の Presented log、
`project.info` / `history.list` の revision / Event と host の OS / GPU。
現時点の実行結果と未検証範囲は [FFI-001 検証](../../docs/testing/ffi-001.md) を参照。

## restricted worker の補助確認

SwiftPM の build service が sandbox 外への書き込みを要求する環境向けに、
`scripts/check_ffi_swift.py` は compiler を直接呼び、同じ core / harness / benchmark を compile する。
XCTest target が包む共通の throwing checks を実行するが、`swift build` / `swift test` の成功判定には使わない。

```sh
python3 scripts/check_ffi_swift.py --swiftc /path/to/toolchain/usr/bin/swiftc --sdk /path/to/MacOSX.sdk
```

