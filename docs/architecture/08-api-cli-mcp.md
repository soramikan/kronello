# 08 API・CLI・MCP・エージェント

GUI・CLI・MCP は同じ Command / Query API を使う（[ADR-0001](../adr/0001-shared-command-query-api.md)）。以下の名前・引数はすべて提案であり、実装済みではない。

## 読み取り API

| API | 内容 |
|---|---|
| `capabilities.get` | 対応ノード、補間、出力、色、GPU 処理経路、検出した codec / hwaccel |
| `scene.query` | 範囲・タグ・種類・ID で検索。ページング |
| `property.schema` | 型、単位、アニメーション可否、参照可能段階 |
| `property.sample` | 指定時刻列で値、source、modifier 結果を返す |
| `scene.explain` | 親変換、マスク、opacity、時刻範囲、欠落資産などを診断 |
| `render.explain` | 使用経路、CPU / GPU 転送、中間メモリ、キャッシュ再利用を報告 |
| `preview.render` | フレーム・短区間・コンタクトシートの成果物を生成 |
| `project.validate` | 構造・文字・資産・機能・性能予算の診断を返す |

## 変更 API

`composition.create`、`scene.node.add`、`scene.parent.set`、`animation.keyframes.upsert`、`expression.bind`、`template.instantiate`、`template.inputs.set`、`instance.retime` 等の型付き操作を transaction へ格納する。
GUI からも同じ操作を使う。

```text
inspect -> draft operations -> edit.plan -> preview(candidate snapshot)
        -> validate -> edit.apply -> render.submit -> job.get -> artifact.get
```

- `base_revision`、`idempotency_key`、`plan_hash`、`policy` を明示する。
- 同一キーの異なる payload は拒否し、同一要求の再送は同一結果を返す。
- 計画後に文書が変わった場合は競合とし、勝手に古い計画を適用しない。
- 大きな計画の validation / compile は commit の前に行うが、commit 時に基準 revision を再確認する。

revision 照合と idempotency の記録は `.koma` 内で行うため、別プロセスからの再送や競合にも同じ規則が適用される（[09 保存と同時編集](09-storage-concurrency.md)）。

## 操作例（提案 CLI）

```bash
koma template instantiate \
  --project demo.koma \
  --template lower_third_ja@1.0.0 \
  --inputs inputs.json \
  --duration 8s --plan-out plan.json

koma edit apply --project demo.koma --plan plan.json --json

koma preview render --project demo.koma \
  --composition comp_lower_third \
  --times 0s,0.2s,0.4s,4s,7.7s,7.9s \
  --quality final --out-dir ./preview --json

koma validate --project demo.koma --profile delivery --json

koma render --project demo.koma --profile hevc-4k \
  --out ./output.mp4 --wait --events ndjson
```

- 標準出力は JSON / NDJSON、標準エラーはログ。
- 非対話モードでは質問せず、必要な権限・入力がなければ型付きエラーを返す。

## MCP

- プロトコル対応版を交渉し、JSON Schema と structuredContent で構造化結果を返す。
- クライアント接続状態に暗黙の対象 Project を保持しない。対象は毎回の要求で明示する。
- 長時間レンダーは永続ジョブにし、MCP 接続の寿命に依存させない。ジョブの実行主体は未決（[OQ-03](../open-questions.md)）。

## 安全性

- 素材の文字列や字幕は命令ではなくデータ。
- 通常操作に shell、任意 FFmpeg 引数、外部 URL fetch を混在させない。
- WASM 拡張を導入する場合も WASI 権限を原則与えず、fuel / epoch、メモリ、host call の制限を別々に設定する。
- WASM の CPU 命令制限は、そこから発行した GPU 処理時間を制限するものではない。
- 未知のシェーダーやネイティブプラグインは別信頼区分にし、初期の自動化は組み込みノードに限定する。
