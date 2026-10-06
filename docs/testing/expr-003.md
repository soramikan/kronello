# EXPR-003 固定 DataAsset・過去 sample・連続 noise

状態: `done`（2026-10-06、root受け入れ済み）。設計: [ADR-0102](../adr/0102-bounded-temporal-expression-assets.md)。
人間向け構文・parser/UIはOQ-17 / EXPR-002の対象であり、このAST実装で完了としない。

## 固有条件の証拠

- `cargo test -p kronello-eval --test expressions --locked`: 既存9件を含む12件成功。
  `PropertySample` は動的な非負秒のlookback、root時刻から減算後のinstance mapを使う。
  root2の現在値が範囲外でも過去1の有効値を取得でき、現在の値を先に読み出さない。
  逆順/再試行・1ns丸め境界・負lookback拒否・past self cycle・nested命令/sample/メモリ予算を確認する。
  `ContinuousNoise` はlattice境界の左右で連続、固定seed・同一時刻の再試行は一致し、
  coordinate範囲外と旧AST版を拒否する。tableの静的依存・行境界/非整数・欠落/hash変更を拒否する。
- `cargo test -p kronello-model --test expression_data --locked`: 1件成功。
  typed table shape、1MiB上限、hash一致、ID除外、重複ID拒否、未知version保持と実行拒否を確認する。
- `cargo test -p kronello-service --test expressions --test audio_analysis --locked`: 4件と3件成功。
  共有JSON EditPlan/Applyとidempotent再送からtable+past samplingを保存し、固定snapshotの
  再serializeと実CPU frameを任意順で描画する。alpha画素は `0.1 + root_time/2` と一致する。
  連続noiseも実CPU frameのalphaが同じSceneIR値になり、再試行pixelが一致する。
  table hash変更、v3-under-pin2、資産欠落、noise範囲外はtyped失敗し、sampleと最終renderで
  代替値を返さない。AUDIO-001の版2 AST・旧pin2描画、pin省略の意味1と拒否を維持する。

最新の個別結果: `target/m5-acceptance/expr003-final-targeted.log`（全20件成功、exit0）。
model/eval/serviceのall-targets Clippy `-D warnings`もexit0（`expr003-clippy.log`）。
固定tableはProject内の `expression_data_assets` に保存し、式評価へ渡すtyped入力を
snapshotから解決する。外部path、最新のstore、clock、非固定乱数を式評価へ渡さない。

## 実CLI/MCP入口の相互運用

以下のコマンドを実行した。

```sh
python3 scripts/verify_expr003.py \
  --binary target/m5-acceptance/expr003-transports/bin/kronello \
  --mcp-binary target/m5-acceptance/expr003-transports/bin/kronello-mcp \
  --output-root target/m5-acceptance/expr003-transports-v2
```

再実行は出力先を新しいdirectoryへ変更する。既存証拠を上書きしない。
実CLI/MCP入口でexit0（2026-10-06、CPU reference）。scriptは実行前に両binaryを
専用 `bin` へコピーしSHA256を記録する。共有targetで別buildが起きても実行入力を変えない。

`expr003-transports-v2/report.json` と各要求/応答JSONが証拠。保存・reopen（MCP再起動、
CLIは各要求で新process）・query・EditPlan/Applyの再送が一致した。
時刻 `[1/2,0,1/4,1/2]` にtable+pastとnoiseを各4回描画し、各2048 pixelsの
linear/display全要素・snapshot hash・revision・pinsがCLI/MCP間で完全一致した。
同時刻の再試行も全画素一致。expression pinは3。
旧AST2への新nodeは `UNSUPPORTED_FEATURE`、欠落DataAssetは `EVALUATION_ERROR` で
両入口が同じtyped拒否となり、保存状態を変更しない。命令budget0の保存後はqueryと
最終frameが両入口で `EXPRESSION_BUDGET_EXCEEDED` となり、代替frameを返さない。
初回scriptの旧版エラー期待を `INVALID_EDIT` としていた失敗ログも保持した。
実際の契約は `UNSUPPORTED_FEATURE` であり、そのコードで再検証した。production変更はない。

## 予算と互換性

`PropertySample` は同じqueryの Usageをnested評価へ渡し、親の小さい式budgetにも
nested増分を課金する。版3では依存scheduleのkey/stack/memo容量も構築前に保守的課金する。
`DataAssetCell` は選択rowだけをindex参照し、column key処理とclone payloadを課金する。
noiseは2つのlattice hashと補間、REPEATの明示seed/context bytesを含む処理を課金する。
要求Propertyごとの予算は独立であり、batch/request順や過去queryのcacheに依存しない。

新snapshotは能力3を固定し、既存能力1/2はその版以下の文書を実行する。未知能力はopaque保存できるが実行しない。
REPEAT contextがないinstanceの旧Noiseのhash入力bytes/出力を維持し、明示repeat seedのみ追加する。

## 統合受け入れ

2026-10-06のREPEAT/EXPR統合checkpointでschema/Swift再生成・一致検証、fmt、
workspace all-targets Clippy `-D warnings`、`cargo test --workspace --locked` がすべてexit0。
全121 suite・805件成功・失敗0・ignored 41件。API 12件（全44 operation）とnle_schema 1件も成功。
証拠は `target/m5-acceptance/workspace-repeat-expr.log`、`clippy-repeat-expr.log`、
`schema-repeat-expr.log`、`swift-check-repeat-expr.log`、`fmt-check-repeat-expr.log`。
ignoredの専用環境試験全件を実行したという意味ではない。
rootが全固有条件・統合結果を確認し、受け入れを完了した。[M5統合記録](m5-acceptance.md)を参照。mainへの統合やM5全体完了とは区別する。
