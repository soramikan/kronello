# ADR-0056: native FFI の非同期 worker と SwiftPM 境界

- 状態: 採用
- 日付: 2026-10-04

## 背景

ADR-0014 / ADR-0031 は同一プロセス C ABI + 共通 JSON と native surface を定めた。
FFI-001 では ownership、実行 thread、Swift 型生成と検証専用 package の具体化が必要。
GUI-001 の画面設計・UI state 保存は含めない。

## 決定

- `kronello-ffi` は `rlib`（Rust tests）と `cdylib`（Swift）を生成する。
  macOS install name は `@rpath/libkronello_ffi.dylib`。ローカル SwiftPM build script が library / CLI を `apps/macos/Libraries` に配置する。
- C ABI は9関数。整数 handle / request ID、UTF-8 buffer、primitive size / bool、opaque CAMetalLayer pointer だけを渡す。
  wgpu / SQLite / Tokio の型・Rust object layout は公開しない。
- open は thread を起動するだけで、completion 0 に共有 `project.info` を返す。
  以後の作品要求は CLI/MCP と同じ明示 project path と共有 strict Request decoder / Service を通す。
  detached job に必要な同じ版の CLI worker は open 時の明示 path で供給し、未指定の submit は `WORKER_EXECUTABLE_REQUIRED`。
- 各 session は一つの専用 OS thread で FIFO work を実行する。compile、disk、GPU、job query を caller thread で実行しない。
  request queue / 未回収 completion は最大64件。超過は enqueue の0で表す。
  非 blocking poll と `kronello_free` で ownership を明確にし、外国語 callback の寿命・再入を扱わない。
- poll の `response_json` は共有 Response の JSON テキスト。transport が巨大な未知数値を decode / encode して丸めない。
  notification は `revision_changed` / `job_progress` と共有 Response テキスト。
  idle 250 ms と work 終了後に外部状態を調べ、同種通知を最新 snapshot に集約する。
- close は join しない。新規 work を止め、受け付け済み work が終了後に layer / surface / output queue を解放する。
  Swift の cancel / timeout は受け付け済み編集を取り消さない。
- CAMetalLayer を caller thread で retain し、Rust worker が `SurfaceTargetUnsafe::CoreAnimationLayer` を作る。
  surface に適合する Metal adapter で GpuContext を構築する。
  service が既存 snapshot / font policy と scene / DAG compiler を共有し、GPU の既存 lowering から texture を作る。
  画像 readback はせず、既存 shader の4-byte validation status だけを読む。
  native preview は黒背景の SDR sRGB。HDR tone mapping を決定するものではない。
- `scripts/generate_swift_api.py` は Python stdlib だけを使い、公開 API schema の全 `$defs`、inline object、union、enum から Codable 型を生成する。
  schema SHA-256 を記録し `--check` で再現性を確認する。既知 tag を schema から読んで union を dispatch する。
  Project の未知 field / 明示 null を保持する。型の意味的制約は Rust が検証する。
  arbitrary JSON number の typed carrier は Foundation Decimal。これを超える未知数値の保持は rawCall を使う。
- SwiftPM package 名 `Kronello`、tools 5.10、macOS 14。
  CKronelloFFI / KronelloCore / KronelloPreviewHarness / KronelloJSONBenchmark / KronelloCoreTests の flat list を持つ。
  preview harness は検証専用の window のみ。KronelloDesign / Kronello の実装は supervisor の別タスクとする。
- workspace の `unsafe_code=forbid` は Cargo の部分上書きができないため、
  既存 kronello-framebridge と同じ lint mirror を FFI crate に限って用い、unsafe_code だけ allow にする。
  純粋 crate は変更しない。unsafe は C buffer、returned allocation、retained layer、surface 作成とその Send 条件のみに限定する。

## 影響

entry point に編集の意味や専用 ProjectStore を持たず、revision / Event / idempotency を共通 service に委ねる。
Swift 型・C header・ABI は再生成チェックと署名テストでレビューできる。
定期 query の I/O と JSON encode/decode のコストは残る。FFI-001 の計測値は合否性能目標を確定しない。
release app bundle / signing、Windows / Linux surface、実 GUI、候補 snapshot の drag preview、HDR 表示は後続範囲。

## 関連

- [ADR-0014](0014-native-gui-in-process-ffi.md)
- [ADR-0029](0029-public-json-schema.md)
- [ADR-0031](0031-ffi-c-abi-json.md)
- [10 デスクトップ GUI](../architecture/10-desktop-gui.md)
- [FFI-001 の検証](../testing/ffi-001.md)

