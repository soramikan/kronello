# 08 API・CLI・MCP・エージェント

GUI・CLI・MCP は同じ Command / Query API を使う（[ADR-0001](../adr/0001-shared-command-query-api.md)）。M1 CLI-001 と M2 SERVICE-001 / API-001 / TEMPLATE-001 / MEDIA-001 / MCP-001 の実装範囲を次節に示す。それ以外の後続 API・CLI は提案であり、実装済みではない。

## M1 CLI-001 の実装範囲

`kronello-service` は同期 Command / Query の入口を提供し、`kronello-cli` の binary `kronello` は transport adapter とする。以下の 6 操作を実装した。編集計画・適用・Undo・最小の history.list は後述の SERVICE-001 で実装した。公開 API schema と構造化 query は後述の API-001 で実装した。永続ジョブ等は引き続き提案である。MCP stdio は後述の MCP-001 で実装した。

| service `Request.operation` | CLI subcommand | payload / 応答 |
|---|---|---|
| `project.create` | `project create` | `project`（新規 `.kronello` path）、`document`（公開 Project）→ ProjectInfo |
| `project.import` | `project import` | `project`、`base_revision`（10進文字列）、`document` → ProjectInfo |
| `project.export` | `project export` | `project` → revision と公開 Project |
| `project.info` | `project info` | `project` → ID、name、revision、構造 / 意味版、content hash、既知 Composition ID |
| `render.frame` | `render frame` | `input`、`time` → FrameMetadata と row-major float32 `linear` / `display` RGBA 配列 |
| `render.sequence` | `render sequence` | `input`、`range`、`frame_rate`、`output_directory` → SequenceMetadata とディスク上の連番 |

`document` は [公開 schema 1](../../schemas/project-v1.schema.json) の `Project` 型を共有する。request envelope は別の型であり、未知 field と重複 field を拒否する。Project 内の未知内容は store の規約で保持する。`project.create` は一時ファイル内で import / close を完了してから上書き禁止で公開する。`project.import` は既存ファイルを対象とし、明示した revision に一致する場合だけ更新する。project.create / project.import は store の event を記録するが、これら二つの操作の変更計画・再送の冪等性 API は未実装（以下の edit.* は対応済み）。読み取りと render で存在しない project を作成しない。

`RenderInput` は `project`、`composition`（stable UUID）、`region`（origin / extent / pixels）、任意の `profile`（既定は linear Rec.709 / tolerance 0.02 px）、任意の `fonts` を持つ。fonts は `{ "identity": FontRef, "path": "local/file.otf" }` の配列とし、snapshot が必要とする font lock をすべて明示する。hash・face index・family・PostScript 名を照合し、システムフォント探索や外部取得はしない。path は process の作業ディレクトリ基準（絶対 path も可）。有理数は `{ "num": "1", "den": "2" }`、range は `{ "start": ..., "end": ... }` とする。

### 機械向け I/O

- subcommand を指定した場合は、その payload の JSON object を stdin に渡す（`operation` を含めない）。subcommand なしの場合は `operation` を含む完全な service Request を渡す。
- `--request-json 'JSON'` は stdin の代わりに一つの要求を渡す。入力上限は UTF-8 16 MiB。一回の起動につき一つの要求、一つの結果 JSON document と改行を stdout に出力する。NDJSON event stream は未実装。
- 成功は `{ "status": "success", "result": { "kind": "project|export|frame|sequence|plan|edit|history|scene|samples|capabilities|collected", "value": ... } }`、失敗は `{ "status": "error", "error": { "code": "INVALID_REQUEST", "message": "..." } }`。成功の exit code は 0、失敗は非 0。診断は stderr にだけ出力する。`--help` も `USAGE` JSON error と stderr の使用法（非 0）を返す。
- backend の既定は GPU。`--backend gpu` も指定可。adapter / device を作れなければ型付きエラーを返す。GPU 不在時の暗黙の CPU fallback はない。GPU は render 操作でのみ初期化する。
- `--backend cpu-reference` は検証用の float32 参照 backend の明示選択。metadata に `cpu_reference_float32` と記録する。通常の GPU は `wgpu_rgba16f`。両者のビット一致や性能保証は提供しない。
- `render.frame` は有理数の任意時刻を評価して画素を JSON 応答する。画像ファイルが必要な場合は `render.sequence` を使う。連番は新しい directory にだけ出力し、既存成果物を上書きしない。PNG / RGBA16F / metadata の契約は [05 レンダー](05-render-gpu.md) を参照。

安定したエラー code は `INVALID_REQUEST`、`PROJECT_NOT_FOUND`、`PROJECT_EXISTS`、`PROJECT_LOCKED`、`REVISION_CONFLICT`、`UNSUPPORTED_FEATURE`、`FONT_MISSING`、`ASSET_HASH_MISMATCH`、`GLYPH_MISSING`、`ADAPTER_UNAVAILABLE`、`DEVICE_UNAVAILABLE`、`IO_ERROR`、`OUTPUT_IO_ERROR` 等。store / render の既存 code（`UNSUPPORTED_SCHEMA_VERSION`、`RENDER_ERROR` 等）も伝播する。message は診断用であり分岐には code を使う。フォントが欠落・不一致のときも最終出力を代替フォントで続行しない。

### CLI-002: adapter 不在のテスト専用注入

`kronello-service` の GPU backend factory は、`GpuContext` の生成に失敗した場合、`GpuError::AdapterUnavailable` を `ADAPTER_UNAVAILABLE` に変換して処理を終了する。CPU 参照 backend は明示選択した場合だけ呼び出す。factory の失敗時には render 処理へ進まず、連番の output directory・frame・metadata を作らない。

GPU を持たない sandbox でもこの経路を検証するため、service の Cargo feature `test-adapter-unavailable` と `debug_assertions` が両方有効なときだけ、factory が環境変数 `KRONELLO_TEST_ADAPTER_UNAVAILABLE=1` を読み、実際の adapter 取得前に `GpuError::AdapterUnavailable` を返す。CLI の dev dependency がこの service feature を有効にするため、通常の `cargo test` で実 binary の失敗経路を検証できる。CLI にも同名の転送 feature があり、検証ビルドで明示的に有効化できる。注入は GPU factory だけを対象とし、project 操作や明示的な `--backend cpu-reference` には影響しない。

通常の `cargo build -p kronello-cli` は dev dependency の feature を有効にしない。標準の release profile では `debug_assertions` が無効なので、feature を明示的に指定しても環境変数の読み取りと故障注入はコンパイルされない。これはテスト専用の仕組みであり、公開 Request・CLI option・配布用設定には追加しない。service の単体テストでは private factory を直接差し替え、プロセス全体の環境変数を変更せずに既定 GPU 経路の失敗伝播を確認する。[CLI-002 の検証](../testing/cli-002.md) に条件とコマンドを記録する。

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

## M2 SERVICE-001 の実装範囲

`kronello-service` に次の四つの同期操作を追加した。CLI は既存と同じ stdin JSON / `--request-json` の transport を使い、編集状態・競合規則を持たない。公開 envelope の schema は後述の API-001 で正式化した。GUI の入口は後続タスク、MCP stdio は後述の MCP-001 で実装した。

| service operation | CLI | payload / 応答 |
|---|---|---|
| `edit.plan` | `edit plan` | `project`、`base_revision`、`commands` → `kind: plan` の EditPlan |
| `edit.apply` | `edit apply` | 上記に `plan_hash`、`session_id`、`idempotency_key` を追加 → `kind: edit` の保存済み Event |
| `edit.undo` | `edit undo` | `project`、`base_revision`、`session_id`、`idempotency_key`、`event_id` → 新しい Event |
| `history.list` | `history list` | `project`、任意の `since_revision`（既定 `"0"`）/ `limit` / `session_id` → 現在 revision、Event / `undone`、次ページ cursor（API-001） |

`base_revision` / `since_revision` は10進文字列、ID は UUID。Plan / HistoryResult の revision も10進文字列だが、既存 store の Event は JSON 整数の revision を返す。`idempotency_key` は空でない UTF-8 文字列（256 bytes 以下）を明示する。session は呼び出し元が決める。history は一つの SQLite 読み取り transaction で現在文書とイベントを取得し、revision 順に session・変更キー・undo_of・取り消し状態を返す。revision が進む前の receipt 再送でも元の Event 全体を返し、現在文書をその時点へ戻さない。

### 型付き編集操作

`commands` は `EditCommand` の配列。一つの外部 tag を持つ JSON object を各操作とし、未知 field・重複 field を拒否する。たとえば Constant の変更は次の形である（UUID は対象文書の stable ID）。

```json
{"property_source_set":{"object":"adcfcd01-95a0-4292-ba82-61369bf4ac8c","property":"561797a3-732d-406d-8755-57fd074c0050","source":{"kind":"constant","value":{"kind":"scalar","value":2.5}}}}
```

| EditCommand tag | 内容 |
|---|---|
| `property_source_set` | `object`、`property`、`source`（Constant / Curve）。任意の `curve` は同じ ID の AnimationCurve を新規保存・置換する。Curve の参照閉包・型を検証 |
| `keyframe_insert` / `keyframe_upsert` / `keyframe_replace` | `curve` と `key`。それぞれ重複時拒否 / 挿入か置換 / 既存の有理数時刻だけ置換 |
| `keyframe_remove` | `curve` と `time`。存在するキーだけ削除 |
| `node_add` | `composition`、完全な SceneNode の `node`、親内の `index`。親は node.containment_parent |
| `node_remove` | `composition` と NodeId の `node`。所有子孫をまとめて削除。残る参照の適合は候補全体の検証で確認 |
| `node_reparent` | `composition`、NodeId の `node`、新 `parent`（root は null）、`index`。変換親は別管理 |
| `transform_parent_set` | `composition`、NodeId の `node`、変換 `parent`（null 可） |
| `node_reorder` | `composition`、所有 `parent`（root は null）、全子 ID の `order`。欠落・重複・外部の子を拒否 |
| `shape_set` / `text_set` | 完全な既知 `shape` / `text` を stable content ID で保存・置換 |
| `composition_create` | 完全な Composition の `composition`。既存 ID は拒否 |
| `instance_place` | `composition`、CompositionInstance kind の SceneNode の `node`、`index`。definition・入力・TimeMap・seed は共有モデル型 |

生の patch、inverse、changed_keys をクライアントから受け付けない。Expression の設定、Timeline 編集、テンプレートの公開入力 policy、Modifier 編集、ジョブはこの実装の対象外。opaque / 未知意味版の通常編集は引き続き保守的に `UNSUPPORTED_FEATURE` として拒否する。

### 計画・適用と receipt

plan は指定 revision の不変文書から候補を作り、`validate_compositions`、Property descriptor / curve source catalog、Shape / Text content、instance binding を検証する。中間操作ではなく batch の最終候補を検証するため、一つの計画内で content・node・definition を追加できる。plan は文書・revision・履歴を変更しない。型なし object UUID による参照と競合判定を曖昧にしないため、node と project / composition / curve / content の UUID の重複も拒否する。

EditPlan は `project_id`、正規化した `base_revision`、commands、生成 mutations / inverse / changed_keys、候補 `candidate`、`plan_hash` を返す。hash は `plan_hash` を空文字列にしたこの内容全体を、JSON object key 順・compact UTF-8 に正規化して SHA-256 にした小文字 hex。クライアントの path・session・idempotency key は plan identity に含めない。同一 project / revision / commands に対し決定的で、候補を別の計画へすり替えられない。

apply は commands から計画を再構築し、hash と候補の検証を確認する。store の `BEGIN IMMEDIATE` 内で revision を再照合し、文書・Event・inverse・receipt を一緒に commit する。競合や保存失敗は全変更を rollback する。保存 patch の配列参照は UUID `id` を使い、別 Property の後続変更を逆操作で上書きしない。draw order や keyframe の配列は単位として置換する。ID 配列の格納順は意味を持たず、候補は patch 適用後の格納順に正規化する。描画順の正本は root_nodes / child_order。

receipt の canonical payload は operation・正規化 revision・session・commands・plan_hash（Undo では event_id）である。project path は各 `.kronello` 内のキーの名前空間で識別し、payload に含めない。同じキー・同じ payload の再送は revision 判定より先に保存結果を返す。同じキー・異なる payload は拒否する。再送判定は service の事前 lookup に加え、store の書き込み transaction 内でも行い、同時プロセスでも重複 Event を作らない。receipt は既存 `idempotency.payload` 内の `service_payload` と完全な Event で永続化し、SQLite table / user_version を変えない。旧 store caller の receipt と SERVICE-001 の receipt は混同しない。

### Selective Undo の実装

Undo は対象 Event の保存 inverse を現在文書に適用した候補を検証し、新しい revision / undo_of を発行する。Redo は Undo Event を対象とする `edit.undo`。history の `undone` は undo_of chain を末尾から走査し、Undo の取り消しも反映する。compact で失った Event は Undo できず、receipt は残る。

値変更は `(object_id, property_id)` が一致したときに競合する。同じ node の別 Property は独立。共有 curve の編集はその curve の直接消費者すべての Property キーを導出する。構造変更は対象 ID または親コンテナ ID の重なりで競合し、構造変更と値変更は対象 object ID の一致で競合する。reparent は旧・新の親を記録する。content / curve / definition の直接参照を新規作成する操作も resource ID を記録し、後続の参照を残したまま作成元を取り消さない。式・評価 DAG の間接的な依存を競合キーへ拡張しない。

対象より後の未取り消し Event が上記キーに触れれば、一切適用せず `UNDO_CONFLICT`。`error.details.conflicts` は `{event_id, keys}` の配列で、競合した後続 Event のキーを返す。Undo 自体も未取り消し Event であり、同じキーを持つ逆操作も競合対象になる。先の操作を Undo した後に同じ領域を操作する場合は、履歴の最新の Undo / Redo Event を対象にする。

| 実装済み error code | 条件 |
|---|---|
| `INVALID_REQUEST` | request decode 不正、revision 文字列不正、空 / 長過ぎる idempotency key |
| `INVALID_EDIT` | 編集対象欠落、key 時刻重複 / 欠落、型・descriptor・参照・所有順序・循環等の候補不適合 |
| `REVISION_CONFLICT` | plan / apply / undo の基準 revision が古い（同じ payload の保存済み再送は成功） |
| `PLAN_HASH_MISMATCH` | 現在 revision に対する生成 plan と要求 hash が一致しない |
| `IDEMPOTENCY_KEY_REUSED` | 同じ project 内の key が異なる canonical payload で使われた |
| `UNDO_CONFLICT` | 後続の未取り消し Event とキーが重なる。details に Event ID とキー |
| `EVENT_NOT_FOUND` | Undo の対象 Event が存在しない、または compact 済み |
| `EVENT_ALREADY_UNDONE` | 対象 Event は既に取り消されている。Redo はその Undo Event を指定 |
| `UNSUPPORTED_FEATURE` | Expression の設定、opaque / 未知意味版の通常編集 |

既存の `PROJECT_NOT_FOUND` / `PROJECT_LOCKED` / `STORAGE_ERROR` / `IO_ERROR` 等も伝播する。これらの受け入れ条件と再現コマンドは [SERVICE-001 の検証](../testing/service-001.md) に記録する。

## M2 API-001 の実装範囲

`kronello-service` が API-001 の同期 query と MEDIA-001 の素材操作を実装し、CLI は同名の二語 subcommand または `operation` 付き stdin JSON で呼ぶ。GPU / font bytes の初期化は不要。scene.query / property.sample は一つの保存 revision の不変文書を読み、revision を10進文字列で返す。capabilities.get は Project を指定せず実装能力を返す。

| operation / result kind | request | 構造化結果 |
|---|---|---|
| `scene.query` / `scene` | `project`、`composition`、任意の `expand_instances`（既定 false） | duration、design_extent、root keys、所有順の pre-order nodes。node の kind、所有親、変換親、children、local time の `[start,end)` active_range |
| `property.sample` / `samples` | `project`、root `composition`、`keys`、`times` | key ごとの value_type / unit / 型付き Value 配列。values は times の順序を保持し、curve・placement binding 適用後の値 |
| `capabilities.get` / `capabilities` | 空 payload `{}` | api_schema_version 1、engine_version、SemanticVersions、commands、features、effects、backends、検出した media |
| `asset.relink` / `project` | `project`、`base_revision`、`asset`、`search_directory` | hash 一致する素材だけを明示再リンクし、更新後の ProjectInfo を返す |
| `project.collect` / `collected` | `project`、`output_directory` | 元プロジェクトの revision を変えず、移動可能な複製を作り directory / project / asset_count を返す |

scene の key は `{instance_path, node}`。expand_instances を指定すると、placement の authored children より前に参照 definition の roots を展開する。同じ definition の二配置は NodeId が同じでも InstancePath が異なる。definition root の所有親と、明示変換親のない内部 node の変換親は enclosing placement となる。active_range は各 definition の local time の値を保持し、時刻による絞り込みや祖先との区間交差はしない。展開を含む最大 node 数は100000で、超過は `INVALID_REQUEST`。範囲・タグ・種類による検索、scene ページング、評価済み transform の返却は未実装。

sample key は `{ "kind":"node", "instance_path":[], "node":"UUID", "property":"UUID" }` または `{ "kind":"composition", "instance_path":[], "composition":"UUID", "property":"UUID" }`。root composition は要求に明示し、各 path はそこから解決する。time は `{ "num":"1", "den":"2" }` の有理数。keys / times は非空で、その積は100000以下。評価には `kronello-eval` を使い、時刻をフレームに丸めない。composition input は placement override と local TimeMap を含めて解決する。欠落 key・無効時刻・式・有効な Modifier 等の未対応評価を代替値で継続せず、`INVALID_REQUEST` / `EVALUATION_ERROR` / `UNSUPPORTED_FEATURE` 等を返す。units は共有 descriptor の `design_px` / `degrees` / `dimensionless`。

capabilities はコンパイル済み対応範囲を示し、GPU adapter の稼働を保証しない。effects の列挙はこの実装では空。media は MEDIA-001 の `MediaRuntime::load` で実際に読み込んだ FFmpeg の情報を返す。既存の runtime_version / decoders / encoders / hwaccels に加え、schema_version、ffmpeg_version、library_directory、substituted、全 libraries の version / license / configuration、codecs の encoder / decoder / hardware、distribution_eligible、development_only を含む。`Service::with_media_capabilities(MediaCapabilities)` で明示したレポートを渡す場合は再検出しない。未指定時の FFmpeg の欠落・不正な override・ABI 不一致は `FFMPEG_UNAVAILABLE` とし、null や既定ライブラリへ暗黙に戻さない。物理 hardware の稼働保証と配布適格性は区別する（[ADR-0048](../adr/0048-media-native-build-and-asset-verification.md)）。

### 公開 schema と registry

[schemas/api-v1.schema.json](../../schemas/api-v1.schema.json) は Draft 2020-12。Request / Response envelope、全19操作の payload / successful result、型付き EditCommand を共有 Rust 型から生成する。`api_json_schema()` と committed schema の一致を通常テストで確認する。再生成は `cargo run -p kronello-service --example api_schema --locked > schemas/api-v1.schema.json`。版はファイル名・`$id`・`x-api-schema-version` で固定し、既存 Request に必須 version field を追加しない。Project document は従来の公開型 / schema を共有する。

`command_registry()` の CommandDescriptor は name / read_only / request_schema / response_schema を持つ。schema refs は同ファイルの `$defs` を指す。request_schema は operation tag を除いた payload、response_schema は status / kind を除いた successful value。MCP-001 / FFI-001 は同じ registry と execute / execute_json を使用できる。registry の19操作は project.create / import / export / info / collect、asset.relink、render.frame / sequence、edit.plan / apply / undo、history.list、scene.query、property.sample、capabilities.get、template.define / instantiate / set_input / set_duration。project.create / import / asset.relink / edit.apply / undo と template の4操作が mutating。他は project に対し read-only（render.sequence と project.collect は output directory に成果物を作る）。project.collect の read_only は元プロジェクトを変更しないことを表し、複製と hash 検証済み素材を新しいフォルダに保存する。

request envelope・既知 payload は未知 field と重複 field を拒否する。schema に任意 shell、外部 URL fetch、raw FFmpeg args の実行 field は設けない。project / font / output path と assets 内の locator の URI scheme は filesystem access の前に `INVALID_REQUEST` とする。Windows drive path は local path として許す。素材の name / text や未知 Project 内容は不活性なデータであり、命令として実行しない。Project の未知 field 保持と、API envelope の厳格な decode は別の契約である。

### history.list のページング

`since_revision` は exclusive cursor（既定 `"0"`）、limit は既定100、範囲1..=1000。任意の session_id で filter した Event を revision 昇順で返す。続きがあれば `next_since_revision` は最後に返した Event の10進 revision、なければ null。同じ filter で次の要求に cursor を渡す。結果は Event 全体（id / session_id / changed_keys / undo_of 等）、undone、現在 revision を含む。undone はページング・session filter の前に全保持履歴の Undo chain から計算するため、別 session や後続ページの Undo も反映する。各ページは一つの read transaction で整合するが、複数ページ全体を固定する snapshot cursor は提供しない。compact された Event は返さない。

受け入れ条件、CPU 検証と未確認範囲は [API-001 の検証](../testing/api-001.md) を参照する。

## 読み取り API（実装済み範囲は上記参照）

| API | 内容 |
|---|---|
| `capabilities.get` | API-001: engine / semantic versions、command registry、features / effects / backends、media 拡張 |
| `scene.query` | API-001: Composition tree と任意の InstancePath 展開。範囲・タグ検索とページングは提案 |
| `property.schema` | 型、単位、アニメーション可否、参照可能段階 |
| `property.sample` | API-001: runtime key と有理数時刻列を指定し、評価済みの型付き値・単位を返す。source / modifier 中間結果の説明は提案 |
| `scene.explain` | 親変換、マスク、opacity、時刻範囲、欠落資産などを診断 |
| `render.explain` | 使用経路、CPU / GPU 転送、中間メモリ、キャッシュ再利用を報告 |
| `preview.render` | フレーム・短区間・コンタクトシートの成果物を生成 |
| `project.validate` | 構造・文字・資産・機能・性能予算の診断を返す |
| `history.list` | イベント（適用されたコマンド）を revision 順に返す。session、変更したキー、取り消し状態を含む |
| `job.get` / `job.list` | ジョブの状態・進捗・成果物を返す |

## 変更 API

以下の直接操作名は提案。SERVICE-001 の実装は `edit.plan` / `edit.apply` 内の型付き `EditCommand` を使う。TEMPLATE-001 の実装済みの4操作は後述の「TEMPLATE-001 の Command」を参照し、`EditCommand::Template` としても利用できる。

`composition.create`、`scene.node.add`、`scene.parent.set`、`animation.keyframes.upsert`、`expression.bind`、`instance.retime` 等の型付き操作を transaction へ格納する。
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

## 操作例（以下の flag 形式は提案 CLI）

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

## M2 MCP-001 の実装範囲

`kronello-mcp` crate の同名 binary は、同期 stdio の薄い JSON-RPC 2.0 adapter。UTF-8 の一行一メッセージで要求・応答を交換し、stdout は protocol のみ、診断・使用法は stderr に出す。各入力の上限は改行を除き16 MiB。stdin の EOF で終了する。HTTP transport、resources、prompts、sampling、MCP task、進捗通知、実行中要求のキャンセルは未実装。`notifications/cancelled` 等の通知は応答も操作実行もしない。

対応版は `2025-06-18` と `2025-11-25`。[MCP lifecycle](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle) に沿って initialize → notifications/initialized → tools/list または tools/call の順に使う。[MCP の版交渉規定](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle#version-negotiation) に従い、対応版の initialize は要求と同じ `protocolVersion` を返し、対応外の文字列なら最新対応版 `2025-11-25` を返して初期化を継続する。クライアントが応答版に対応していなければ接続を切断する判断を行う。いずれも tools capability（listChanged: false）を返す。初期化前および完了通知前の tool 要求は `-32002`。ping は初期化前後とも可能。同一接続の再 initialize は拒否する。

`tools/list` は `kronello-service::command_registry()` の全操作を同名で公開する。固定の MCP 操作一覧は持たず、MEDIA-001 等で registry と service Request を拡張すれば同じ経路で公開される。request_schema / response_schema が指す公開 API 型から `api_json_schema()` を生成し、各 schema に必要な `$defs` の参照閉包を同梱する。外部 schema fetch は不要。inputSchema は operation を除いた service payload と同一。outputSchema は成功値の schema と公開 Response の error branch の union で、エラー結果も schema に適合する。`_meta.kronello` に元の schema refs と readOnlyProject を返す。read_only は作品に対する性質であり、render.sequence の成果物書き出しも含むため MCP の readOnlyHint に置き換えない。

`tools/call` の name を operation tag に変換し、arguments の生 JSON を `Service::execute_json` に渡す。重複 field・未知 field の拒否、revision、idempotency、Undo、local path policy 等は service と共有する。成功の structuredContent は service result.value、失敗は `{ "status":"error", "error": ServiceError }` と isError: true。[対応2版の tools 契約](https://modelcontextprotocol.io/specification/2025-11-25/server/tools) に従い、両方で同じ structuredContent を serialize した TextContent も必ず返す。成功値への部分結果や代替値は加えない。未知 method / tool や不正な JSON-RPC envelope は protocol error、既知 tool の入力不正・実行失敗は tool error とする。

作品を対象とする全操作は毎回 `project` を明示する（render は `input.project`）。省略は INVALID_REQUEST。接続は初期化状態だけを持ち、暗黙の current project / session、project.open tool、接続に跨る ProjectStore を持たない。`capabilities.get` は共有 API の空 payload `{}` のまま、対象作品を持たないグローバル discovery とする。編集 session_id は service payload に毎回明示する。

backend は CLI と同じ GPU 既定、`--backend gpu|cpu-reference` で明示選択する。GPU 不在時の暗黙 CPU fallback はない。同期 render.frame / render.sequence は既存 service 操作を公開する。長時間レンダーを永続ジョブにする API と、接続寿命に依存しない worker は JOB-001 の提案であり、MCP-001 では実装しない（[14 ジョブ](14-jobs.md)）。検証方法と境界は [MCP-001 の検証](../testing/mcp-001.md)。

実行例:

```sh
cargo run -p kronello-mcp --locked -- --backend cpu-reference
```

クライアントから stdin へ送る例（各行の応答を読み取る）:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"example","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project.info","arguments":{"project":"/private/tmp/demo.kronello"}}}
```

## 安全性

- 素材の文字列や字幕は命令ではなくデータ。
- 通常操作に shell、任意 FFmpeg 引数、外部 URL fetch を混在させない。
- WASM 拡張を導入する場合も WASI 権限を原則与えず、fuel / epoch、メモリ、host call の制限を別々に設定する。
- WASM の CPU 命令制限は、そこから発行した GPU 処理時間を制限するものではない。
- 未知のシェーダーやネイティブプラグインは別信頼区分にし、初期の自動化は組み込みノードに限定する。

### TEMPLATE-001 の Command

`template.define` / `template.instantiate` / `template.set_input` / `template.set_duration` は同名の二語 CLI subcommand で呼ぶ。すべて `project`、`base_revision`、`session_id`、`idempotency_key` を持ち、成功時は `kind: edit` の Event を返す。定義・配置・入力・尺の payload と制約は [07 テンプレート](07-templates.md) を参照。これらも registry と公開 schema、revision・idempotency・Undo の共通経路を使う。
