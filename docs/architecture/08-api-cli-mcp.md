# 08 API・CLI・MCP・エージェント

GUI・CLI・MCP は同じ Command / Query API を使う（[ADR-0001](../adr/0001-shared-command-query-api.md)）。M1 CLI-001 の実装範囲を次節に示す。後続の API・CLI・MCP は提案であり、実装済みではない。

## M1 CLI-001 の実装範囲

`kronello-service` は同期 Command / Query の入口を提供し、`kronello-cli` の binary `kronello` は transport adapter とする。以下の 6 操作を実装した。それ以降の章の編集計画、Undo、永続ジョブ、MCP 等は引き続き提案であり、SERVICE-001 / API-001 の完了を意味しない。

| service `Request.operation` | CLI subcommand | payload / 応答 |
|---|---|---|
| `project.create` | `project create` | `project`（新規 `.kronello` path）、`document`（公開 Project）→ ProjectInfo |
| `project.import` | `project import` | `project`、`base_revision`（10進文字列）、`document` → ProjectInfo |
| `project.export` | `project export` | `project` → revision と公開 Project |
| `project.info` | `project info` | `project` → ID、name、revision、構造 / 意味版、content hash、既知 Composition ID |
| `render.frame` | `render frame` | `input`、`time` → FrameMetadata と row-major float32 `linear` / `display` RGBA 配列 |
| `render.sequence` | `render sequence` | `input`、`range`、`frame_rate`、`output_directory` → SequenceMetadata とディスク上の連番 |

`document` は [公開 schema 1](../../schemas/project-v1.schema.json) の `Project` 型を共有する。request envelope は別の型であり、未知 field と重複 field を拒否する。Project 内の未知内容は store の規約で保持する。`project.create` は一時ファイル内で import / close を完了してから上書き禁止で公開する。`project.import` は既存ファイルを対象とし、明示した revision に一致する場合だけ更新する。これらの操作は store の event を記録するが、変更計画や再送の冪等性 API は未実装。読み取りと render で存在しない project を作成しない。

`RenderInput` は `project`、`composition`（stable UUID）、`region`（origin / extent / pixels）、任意の `profile`（既定は linear Rec.709 / tolerance 0.02 px）、任意の `fonts` を持つ。fonts は `{ "identity": FontRef, "path": "local/file.otf" }` の配列とし、snapshot が必要とする font lock をすべて明示する。hash・face index・family・PostScript 名を照合し、システムフォント探索や外部取得はしない。path は process の作業ディレクトリ基準（絶対 path も可）。有理数は `{ "num": "1", "den": "2" }`、range は `{ "start": ..., "end": ... }` とする。

### 機械向け I/O

- subcommand を指定した場合は、その payload の JSON object を stdin に渡す（`operation` を含めない）。subcommand なしの場合は `operation` を含む完全な service Request を渡す。
- `--request-json 'JSON'` は stdin の代わりに一つの要求を渡す。入力上限は UTF-8 16 MiB。一回の起動につき一つの要求、一つの結果 JSON document と改行を stdout に出力する。NDJSON event stream は未実装。
- 成功は `{ "status": "success", "result": { "kind": "project|export|frame|sequence", "value": ... } }`、失敗は `{ "status": "error", "error": { "code": "INVALID_REQUEST", "message": "..." } }`。成功の exit code は 0、失敗は非 0。診断は stderr にだけ出力する。`--help` も `USAGE` JSON error と stderr の使用法（非 0）を返す。
- backend の既定は GPU。`--backend gpu` も指定可。adapter / device を作れなければ型付きエラーを返す。GPU 不在時の暗黙の CPU fallback はない。GPU は render 操作でのみ初期化する。
- `--backend cpu-reference` は検証用の float32 参照 backend の明示選択。metadata に `cpu_reference_float32` と記録する。通常の GPU は `wgpu_rgba16f`。両者のビット一致や性能保証は提供しない。
- `render.frame` は有理数の任意時刻を評価して画素を JSON 応答する。画像ファイルが必要な場合は `render.sequence` を使う。連番は新しい directory にだけ出力し、既存成果物を上書きしない。PNG / RGBA16F / metadata の契約は [05 レンダー](05-render-gpu.md) を参照。

安定したエラー code は `INVALID_REQUEST`、`PROJECT_NOT_FOUND`、`PROJECT_EXISTS`、`PROJECT_LOCKED`、`REVISION_CONFLICT`、`UNSUPPORTED_FEATURE`、`FONT_MISSING`、`ASSET_HASH_MISMATCH`、`GLYPH_MISSING`、`ADAPTER_UNAVAILABLE`、`DEVICE_UNAVAILABLE`、`IO_ERROR`、`OUTPUT_IO_ERROR` 等。store / render の既存 code（`UNSUPPORTED_SCHEMA_VERSION`、`RENDER_ERROR` 等）も伝播する。message は診断用であり分岐には code を使う。フォントが欠落・不一致のときも最終出力を代替フォントで続行しない。

### 再現可能な Shape + 日本語 Text デモ

[examples/m1-demo.project.json](../../examples/m1-demo.project.json) は赤い rectangle の position curve と緑の「日本語」Text を持つ。ID と Noto Sans CJK JP の font lock を固定している。font bytes は配布物に追加せず、既存 fixture を使用する。

```bash
python3 scripts/fetch_fixtures.py --offline
cargo build -p kronello-cli --locked
python3 scripts/demo_cli_m1.py --backend cpu-reference --output-root /private/tmp/kronello-m1-cpu-demo
# Metal / Vulkan adapter を持つホストで GPU を確認する。
python3 scripts/demo_cli_m1.py --backend gpu --output-root /private/tmp/kronello-m1-gpu-demo
```

output root は未作成の path を選ぶ。デモは binary を呼び、create → export → import → render.sequence を実行する。0 秒から 1 秒未満を 4 fps で出力し、4 PNG、4 RGBA16F、4 frame JSON、`sequence.json`、project、各要求・stdout・stderr を保存する。file count、全 artifact の byte count / SHA-256、revision、font lock、backend、frame index、時刻による画素 hash の変化を検証する。PNG は supervisor / 利用者が目視できる。

個別の問い合わせ例:

```bash
printf '%s\n' '{"project":"/private/tmp/kronello-m1-cpu-demo/demo.kronello"}' | target/debug/kronello project info
printf '%s\n' '{"operation":"project.export","project":"/private/tmp/kronello-m1-cpu-demo/demo.kronello"}' | target/debug/kronello
```

受け入れテストは `crates/kronello-cli/tests/machine.rs`。built binary を起動し、stdout 全体の JSON parse、非 0 exit と stderr 診断、create / import / export / info、revision conflict / lock、font 欠落 / hash、未対応機能、CPU 連番を検証する。GPU を必要とするテストは `gpu_headless_default_backend_animated_shape_japanese_text_sequence`。service は `Service::with_backend(&dyn RenderBackend)` で backend を注入でき、CLI 固有の作品状態は持たない。 CPU と Apple M1 / Metal の実行結果は [CLI-001 の検証](../testing/cli-001.md) に記録する。

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
| `history.list` | イベント（適用されたコマンド）を revision 順に返す。session、変更したキー、取り消し状態を含む |
| `job.get` / `job.list` | ジョブの状態・進捗・成果物を返す |

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

### Undo

`edit.undo` は、指定したイベントの逆操作を新しいコマンドとして発行する（[ADR-0026](../adr/0026-selective-undo.md)）。他の変更 API と同じく `base_revision` と `idempotency_key` を取る。対象イベントが変更したキーに、それより後の取り消されていないイベントが触れていれば `UNDO_CONFLICT` で拒否する。詳細は [09 保存と同時編集](09-storage-concurrency.md)。

### 素材とプロジェクト

| API | 内容 |
|---|---|
| `asset.relink` | 指定したフォルダから content hash が一致するファイルを探し、素材のパスを更新する |
| `asset.replace` | 素材を内容の違うファイルへ明示的に差し替える |
| `project.collect` | プロジェクトの複製と素材を、相対パスでまとめたフォルダとして書き出す |
| `project.export` / `project.import` | 公開 JSON スキーマでプロジェクトを書き出す・取り込む |
| `history.compact` | 指定した revision より前の履歴を切り捨てる |

要求と応答の JSON は、版付きの公開スキーマに従う（[ADR-0029](../adr/0029-public-json-schema.md)）。

文書値の型定義は共有するが、要求・応答の envelope をプロジェクト全体と同じ形にする規約ではない。構造・意味・実行能力を分けて判定し、未知内容の保持と安全な変更の条件は [ADR-0045](../adr/0045-snapshot-compatibility-boundaries.md) に従う。`capabilities.get` と `project.validate` は render compile / worker と共通の互換性判定を使う。

### ジョブ

`render.submit` はジョブを記録して worker プロセスを切り離して起動し、すぐに job ID を返す。`job.cancel` で取り消し、`job.resume` で中断したジョブを再開し、`job.prune` で古いジョブディレクトリを掃除する。詳細は [14 ジョブ](14-jobs.md)。

revision 照合と idempotency の記録は `.kronello` 内で行うため、別プロセスからの再送や競合にも同じ規則が適用される（[09 保存と同時編集](09-storage-concurrency.md)）。

## 操作例（提案 CLI）

```bash
kronello template instantiate \
  --project demo.kronello \
  --template lower_third_ja@1.0.0 \
  --inputs inputs.json \
  --duration 8s --plan-out plan.json

kronello edit apply --project demo.kronello --plan plan.json --json

kronello preview render --project demo.kronello \
  --composition comp_lower_third \
  --times 0s,0.2s,0.4s,4s,7.7s,7.9s \
  --quality final --out-dir ./preview --json

kronello validate --project demo.kronello --profile delivery --json

kronello render --project demo.kronello --profile hevc-4k \
  --out ./output.mp4 --wait --events ndjson
```

- 標準出力は JSON / NDJSON、標準エラーはログ。
- 非対話モードでは質問せず、必要な権限・入力がなければ型付きエラーを返す。

## MCP

- プロトコル対応版を交渉し、JSON Schema と structuredContent で構造化結果を返す。
- クライアント接続状態に暗黙の対象 Project を保持しない。対象は毎回の要求で明示する。
- 長時間レンダーは永続ジョブにし、MCP 接続の寿命に依存させない。ジョブは切り離した worker プロセスが実行する（[14 ジョブ](14-jobs.md)）。

## 安全性

- 素材の文字列や字幕は命令ではなくデータ。
- 通常操作に shell、任意 FFmpeg 引数、外部 URL fetch を混在させない。
- WASM 拡張を導入する場合も WASI 権限を原則与えず、fuel / epoch、メモリ、host call の制限を別々に設定する。
- WASM の CPU 命令制限は、そこから発行した GPU 処理時間を制限するものではない。
- 未知のシェーダーやネイティブプラグインは別信頼区分にし、初期の自動化は組み込みノードに限定する。
