# REPEAT-001 検証記録

状態: `done`（2026-10-06、root受け入れ済み）。本記録は作業木の検証を表し、main への統合や M5 全体の完了を主張しない。

契約は [ADR-0103](../adr/0103-shared-repeater-and-explicit-materialization.md)。`Project.repeaters` の共有 Source、保存済み instance ID / seed / placement、通常 Property 編集、明示 `RepeaterExpand` を共有 API に接続した。`expanded_source` は個別 Source を参照するため、編集 Source や元 template は書き換えない。

## 実行済み

- `cargo check -p kronello-service --locked`: PASS。
- `cargo test -p kronello-service --test repeater --locked`: 3 / 3 PASS（最終 source の再実行も PASS）。
- `cargo test -p kronello-service --test template --locked`: 既存 9 / 9 PASS。
- `cargo clippy -p kronello-service -p kronello-render -p kronello-model -p kronello-template --all-targets --locked -- -D warnings`: PASS。
- `python3 -m py_compile scripts/verify_repeat001.py`: PASS。

`expand_is_explicit_independent_seed_preserving_and_undoable` は nested Composition の共有 Source を実プロジェクトとして保存し、任意時刻 0 / 1/2 / 3/4 秒の CPU pixels を expand 前後で完全一致比較する。元 Composition / Shape / immutable TemplateDefinition を保持し、instance ID / seed を保持する。コピーした geometry だけの通常 Property 編集、永続的 idempotent replay、Undo 復元を確認する。競合のない expansion undo は全コピーを除去し、後続個別編集がある場合は `UNDO_CONFLICT` で保護する。

`stable_seed_context_reordering_pins_and_typed_rejection` は保存 instance の配列順を反転して、ID ごとの値・world transform が変わらないこと、seed 変更が Noise 結果を変えること、pin 不在・未知版・重複 identity・cycle を拒否することを確認する。

`nested_template_variant_inputs_layout_and_time_survive_materialization` は日本語 portrait variant と DataTable による text / accent override、ink-bound band、max-lines、Noise を含む Source を使う。protected loop の intro / middle / outro を含む 1/5 / 1/2 / 4 / 39/5 秒の実 pixels が expand 前後で完全一致する。コピーした文字の編集後も動的 band 規則を使い、元 TemplateDefinition / TemplateInstance と元 Source を保持する。

## 実 transport / GPU

`scripts/verify_repeat001.py` は CLI / MCP バイナリを証拠ディレクトリへコピーし、実 stdio JSON / MCP JSON-RPC を使用する。`examples/m5-repeat.project.json` と `examples/m5-repeat-template.project.json` を用いて、plan / persisted apply / idempotent replay / export / undo / typed refusal を両入口で比較する。CPU と Metal は同じ revision / snapshot hash / semantic pins / rational time を使い、4096 pixels の linear と display 全 RGBA を比較する。許容最大絶対誤差は 0.002。expand 前後の CPU pixels は完全一致を要求する。

```sh
cargo build -p kronello-cli -p kronello-mcp --locked
python3 scripts/verify_repeat001.py --output-root target/m5-acceptance/repeat001-transports
```

host Metal が必要。sandbox 内の preflight は `ADAPTER_UNAVAILABLE` で GPU 検証を終了した。2026-10-06 の実 host Apple M4 / Metal 4 実行は PASS。17 frame cases の CPU CLI / MCP 結果は完全一致し、全 RGBA の Metal 最大絶対誤差は linear `0.00078726`、display `0.00082290`（許容 `0.002`）。generic nested Noise と nested TemplateInstance の両 Source で、expand 前後の各 rational time の CPU linear / display pixels が完全一致した。instance の Property 操作、永続 replay、元 Source / template / instance ID / seed 保持、expand undo、同一 `REPEATER_SOURCE` 拒否と無変更を実 transport で確認した。

使用 CLI SHA-256 は `9c5855f571cae7827ef16f85d38ba65d9efcfb58abb1d5f44aba4dd6cf3b8fe5`、MCP は `92f649094ce613cc53bd1c2a035b92d120c590700f9b19015c548101fbf9daf1`。コピーした両バイナリの SHA-256 は実行前後一致。request / response と SHA-256 は `target/m5-acceptance/repeat001-transports/`、集計は `report.json` に保存する。

## 統合受け入れ

公開schema/Swift再生成・一致検証、fmt check、workspace all-targets Clippy、workspace testがすべてexit 0。121 suites、805 passed / 0 failed / 41 ignored、API 12試験（44操作）とNLE schema 1試験も成功。rootが固有条件と統合結果を確認し、受け入れを完了した。ログと過去796件のcheckpointは[M5統合記録](m5-acceptance.md)を参照。
