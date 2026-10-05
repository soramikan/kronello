# JSON field 順の独立性

## 原因と修正

2026-10-05、GUI-001 で一度観測された `INVALID_REQUEST` / `invalid type: map, expected f64` を model / service / CLI 実 process / MCP stdio tool / FFI worker で再現した。
`serde_json` の `arbitrary_precision` は Cargo feature unification により workspace 全体で有効になる。
Serde の adjacent tag で `value` が `kind` より先に来ると、payload は `Content` に buffer される。
小数は `{"$serde_json::private::Number":"..."}` という内部 map になるため、その後の f64 decode が失敗する。
tag を先に読める場合は具体型の JSON decoder が使われ、成功する。

[ADR-0046](../adr/0046-store-format-and-location-policy.md) は、未知の大きな整数・数値の綴りを保持し、それを含む compact JSON の SHA-256 を document identity にする。
`arbitrary_precision` の削除はこの保証を変えるため採用しない。
`kronello-model::wire::Adjacent` は strict な object と `Box<RawValue>` を読み、tag に対応する具体型へ payload を decode する。
入れ子の duplicate field も raw JSON に残り、既存の strict decoder が拒否する。unit variant は payload 省略または null を受け付ける。
serialization / JsonSchema / schema_version / semantic_version / SQLite user_version / hash 計算は変更していない。

## 型の監査

対象 crate: model / time / animation / service / store / eval / mcp / ffi / cli / jobs / template / render。
`#[serde(tag = ...)]`、`untagged`、`flatten`、custom `Deserialize`、`try_from` を確認した。

| 型・経路 | 判定 |
|---|---|
| model `Value` | Scalar / Angle / Vec2 / Vec3 / Color / Path / DataTable の直接・入れ子数値が影響。raw adjacent decode に変更 |
| model `PathSegment` | MoveTo / LineTo / QuadTo / CubicTo の数値が影響。raw adjacent decode に変更 |
| model `PropertySource<T>` | Constant の数値・入れ子 `Value` が影響。raw adjacent decode に変更。遅延 decode のため `T: DeserializeOwned` |
| model `ValueRange` | NumericBound の FiniteF64 が影響。raw adjacent decode に変更 |
| model `CurveInterpolation` | Cubic の TimeBezier control が影響。raw adjacent decode に変更 |
| model `NodeKind` | CompositionInstance の input_bindings の数値が影響。raw adjacent decode に変更 |
| model `ShapeGeometry` / `EffectParameters` | variant payload は PropertyId / ContentId の参照のみ。変更なし |
| model `Gradient` / `SourceRef` | 既存の custom JSON dispatch が Content を避ける。変更なし |
| model `DocumentObject<T>` / `OpaqueObject` / `Project` | untagged / flatten は serialization と schema の属性。既存 custom `unique_fields` / JSON decode で未知数値を保持。変更なし |
| model `Effect` | untagged は serialization と schema の属性。既存 custom Value / JSON decode で未知数値を保持。変更なし |
| model FiniteF64 / Color / Property / TimeBezier / AnimationCurve / DesignExtent / PropertyDescriptor / SchemaKey の wire 変換 | plain struct / primitive / string の具体型 decode。変更なし |
| time `MapWire` / `TimeMap` / `Rational` / Duration / TimeRange / FrameRate / SampleRate | 有理数は num / den の decimal string。数値 Content による失敗はない。正規化・不正時間の拒否は既存どおり。変更なし |
| service `Request` / `ResultData` / `Response` / `JobOutput` / `RenderInput` | 既存 custom RawValue dispatch が Content を避ける。変更なし |
| service `SampleKey` / `InspectionPropertyKey` | UUID / InstancePath / enum のみ。変更なし |
| store `Mutation` | arbitrary JSON は serde_json::Value が private Number を復元する。event / inverse / replay の保存テストで確認。変更なし |
| store `ChangedKey` / render `RenderTarget` | UUID のみ。変更なし |
| mcp `strict::Unique` | decode 前の重複 key 検出用 Visitor。型付き numeric payload を解釈しない。変更なし |
| animation / eval / ffi / cli / jobs / template | 該当する tag / untagged / flatten / custom Content decoder はない。共有 model / service decoder の修正を利用 |

## 回帰テスト

修正前は model の六型それぞれの `serde_json::from_str` と各入口の `json_order_*` テストが失敗した。
model / service は `invalid type: map, expected f64`、CLI / MCP / FFI は `INVALID_REQUEST` を返した。
入口テストは修正後、missing project に対する `PROJECT_NOT_FOUND`、実 project の `edit.plan` 成功・canonical 順と同じ plan、真に不正な要求の `INVALID_REQUEST` を確認する。

| ファイル | 確認内容 |
|---|---|
| `crates/kronello-model/tests/json_order.rs` | 六型の tag 後置、全 adjacent variant、nested vec2 / color / Path / DataTable、instance binding、keyframe / effect / generator / rational time、Project が Known のまま復元されること、unknown / duplicate / nonfinite / 不正型の拒否 |
| `crates/kronello-service/tests/json_order.rs` | Request decode、成功 plan の同一性、keyframe / color command、Response decode、typed invalid errors |
| `crates/kronello-cli/tests/machine.rs::json_order_cli_machine_process` | 実 binary の machine mode stdin |
| `crates/kronello-mcp/tests/stdio.rs::json_order_mcp_tool_call` | 実 MCP process の raw tools/call。再 encoding で入力順を変えない |
| `crates/kronello-ffi/tests/boundary.rs::json_order_ffi_worker_boundary` | kronello_call → worker → poll の JSON ABI |
| `crates/kronello-store/tests/storage.rs::json_order_unknown_number_spelling_and_hash_survive_reopen_and_replay` | u64 超の整数・高精度小数・末尾ゼロの綴り、event / snapshot replay / export / reopen、field 順変更で hash 同一・未知数値の綴り変更で hash 相違 |

再現・確認コマンド（`CARGO_BUILD_JOBS=3`、共有 CARGO_HOME / CARGO_TARGET_DIR、管理された TMPDIR）:

```bash
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py generate
cargo test -p kronello-model -p kronello-service -p kronello-store -p kronello-cli -p kronello-mcp -p kronello-ffi --locked json_order -- --nocapture
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_
python3 scripts/generate_swift_api.py --check
```

workspace suite の前に fixture を準備する。今回の初回実行は既存 CLI job test の `cfr-24-1.nut` 欠落で exit 101 となったため、規定の software media fixtures 9 件を生成・decode して再実行した。外部 Noto font も固定 hash を照合して取得した。

2026-10-05 の最終 working tree で、Rust / Cargo 1.95.0、`CARGO_BUILD_JOBS=3`、`KRONELLO_STATE_ROOT="$TMPDIR/json-order-state"` を使用した。`cargo fmt --all --check`、workspace clippy、上記 CPU workspace test、Swift API generation check はすべて exit 0。workspace test は 570 passed / 1 ignored / 10 filtered out（unit / integration / doc-test の合計）で、追加した `json_order_*` 16 件をすべて含む。公開 schema の Rust generator 一致テストも合格し、schema / GeneratedAPI.swift は変更なし。

GPU / hardware codec はこの修正の検証では実行しない。GUI drag の実画面での再確認と GPU を含む host suite は別の検証である。
