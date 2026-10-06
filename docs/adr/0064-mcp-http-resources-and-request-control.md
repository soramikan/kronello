# ADR-0064: MCP HTTP・明示 resource・request 制御を共有 service に接続する

- 状態: 部分置換（[ADR-0084](0084-per-request-mcp-2026.md): protocol version と transport session。共有 service・明示 project path・security / limit policy は維持）
- 日付: 2026-10-05
- 対象: MCP-002

## 背景

MCP-001 の stdio と共有 registry / schema を維持し、HTTP、resources、prompts、進捗と要求取り消しを追加する。作品の正本と永続ジョブを transport に持ち込まない。ADR-0001 / 0009 / 0025 / 0029 / 0050 の決定は変更しない。

## 決定

### protocol と capability

基準は固定した [2025-11-25 specification](https://modelcontextprotocol.io/specification/2025-11-25)。既存の `2025-06-18` / `2025-11-25` をサポートし、initialize → notifications/initialized を維持する。対応版を要求されたら同じ版を返し、それ以外（`2026-07-28` を含む）の initialize には [版交渉規定](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle#version-negotiation) に従って最新**対応**版 `2025-11-25` を返す。クライアントがその版を理解できなければ切断する。

調査時の最新 `2026-07-28` は未対応。`server/discover` は `-32601` と `error.data.code: UNSUPPORTED_PROTOCOL_VERSION` / `supported` を返す。HTTP の対応外 version header は400と同じ型付き code / 対応版一覧を返す。最新仕様の lifecycle を実装したと表示しない。

広告する capability は tools (`listChanged:false`)、resources (`subscribe:false,listChanged:false`)、prompts (`listChanged:false`) のみ。sampling、MCP tasks、elicitation、completion、resource subscription、list change notification は未対応で、クライアントが capability を広告しても採用しない。該当 method は `-32601`、tool call の `task` field は `-32602`。Kronello の `render.submit` / `job.*` は共有 service の永続ジョブであり、MCP task ではない。

### HTTP と security

[Streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports) の `/mcp` を提供する。既定は stdio、`--http` で `127.0.0.1:8765`、`--bind IP:port` で明示変更。IP literal のみで、名前解決を行わない。port 0 は OS による採番を許し、実 address は stderr の `MCP_HTTP_LISTENING` に出す。

- POST は JSON の単一 message、`Content-Type:application/json`、`Accept:application/json,text/event-stream` を要求する。即時応答は JSON、service work は POST の SSE、受理した notification は空 body の202。GET は405（独立した SSE stream は提供しない）。旧 HTTP+SSE transport、batch、event replay / resumption は未対応。POST stream の切断を request cancellation と解釈しない。
- 成功した initialize に暗号学的乱数の UUID `MCP-Session-Id` を発行する。後続要求は session ID を必須とし、無い場合400、失効 / 未知 ID は404。`MCP-Protocol-Version` は交渉版と一致させる。省略時は session の交渉版が正本。DELETE は204で session を終了する。最大64 session、最終 HTTP request から30分で失効（次のアクセスで回収）。Ctrl-C は listener を閉じて request work を協調停止する。SIGTERM は OS の通常終了であり graceful と保証しない。
- loopback の token は任意。非 loopback は明示 `--auth-token-env NAME` と32 byte以上の可視 ASCII token が必須で、無設定なら bind 前に失敗する。全 method に `Authorization: Bearer ...` を照合し、欠落 / 不一致は401。token を CLI 引数、URL、ログ、stdout に出さない。同じ server の全 session は同じ credential の権限を持ち、session ID だけで認証を代替しない。
- Origin が存在する接続はすべて403。ブラウザ origin の allowlist / CORS は提供しない。loopback は Host を実 bind authority または同 port の localhost に制限し、DNS rebinding を防ぐ。TLS / OAuth / multi-user ACL は内蔵しない。非 loopback を利用する運用では、信頼された TLS terminator、network 制限と credential 管理を明示配置する。token の保持者には OS ユーザーと同じ local Project / asset / output の共有 API 権限がある。path jail や任意ファイル閲覧 API として扱わない。

### resources と prompts

`resources/list` は global な `kronello://schema/api-v1` のみ。text は共有 `api_json_schema()`、MIME は `application/schema+json`。作品や directory を探索しない。

`resources/templates/list` は `kronello://project/{project}/info` と `/snapshot` の二つ。URI の project segment は毎回明示する local `.kronello` locator を一度だけ canonical percent encoding（UTF-8、uppercase hex、unreserved はそのまま）したもの。read は共有 `project.info` の `ProjectInfo` または `project.export` の `ExportResult {revision,document}` を JSON text / `application/json` として返す。無指定、未知 URI / kind、不正 encoding、外部 URL は拒否し、資産や任意 file の fetch はしない。

`prompts/list` は `inspect-project` 一つ。`prompts/get` の arguments は `{project:string}`（必須、未知 / 重複 field は拒否）。固定した user TextContent と、指定 Project の snapshot を含む別の EmbeddedResource の二メッセージを返す。素材・作品名・字幕・text を instruction text に補間しない。素材文字列は untrusted data と明記し、shell、式、外部 URL、FFmpeg 引数として実行しない。prompt を取得しただけでは編集や model sampling を実行しない。クライアント側 model の挙動はこの server が保証できる範囲ではない。

resources / prompts は supervisor と合意した共有 `Service::with_read_only_inspection` policy を選択する。対象は既存 `project.info` / `project.export` の型付き Request / Result で、別の作品モデルは作らない。既存 `ProjectStore::read_snapshot` の READ_ONLY / query_only と snapshot schema 検査を使い、migration、checkpoint、作品内容 / revision の変更、writer / exclusive lock の取得を行わない。SQLite 自体の reader lock、WAL / SHM sidecar の利用・作成はありうる。既存 CLI / tool の open/close、ForceSafe の `PROJECT_LOCKED`、sidecar cleanup 契約は変更しない。

### progress と cancellation

stdio / HTTP は同じ `Connection` dispatcher を通す。接続に保持するのは readiness、交渉版、active request ID / token と control のみ。作品、編集 session、ProjectStore は保持しない。process の service work は最大3同時、接続の outstanding work は最大32。ID は integer / string を区別し、active ID と active progressToken の重複は拒否する。入力は両 transport とも16 MiB、未知の typed field と全 JSON object の重複 member を拒否する。拡張用 `_meta` / capability map と作品の opaque data は、既存 schema の拡張規約を保持する。

[progress](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/progress) は要求の `_meta.progressToken`（string / integer）があるときだけ、受付0と処理完了1を通知する。sequence rendering は共有 service `ExecutionControl` と既存 renderer の checkpoint を使い、完了 frame 数を total frame 数に対して通知する。中間 frame 通知は100 ms以上の間隔、値は厳密に増加し、最終値は間引かない。これは request の進行で、detached job の進捗 subscription ではない。

[notifications/cancelled](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation) は接続内の `requestId` を照合する。未知、完了済み、initialize の取消は無応答で無視。queued work は semaphore 待ちを解除して実行しない。service は dispatch 前、sequence の frame 境界と manifest 公開前に cancellation を確認する。取り消し後の response / 新しい progress は出さない。既に送信した response と cancellation の race は正常な競合として扱う。

単一 frame、native codec call、SQLite transaction を途中で強制停止しない。commit 済み編集、最後の checkpoint を通過した成果物、投入済み job は rollback しない。この場合も取消で response を失うことがあるため、共有 revision / receipt / job ID で確認する。MCP の取消や EOF、DELETE、session 失効、server 終了を `job.cancel` に変換しない。job を停止するには共有 service の `job.cancel` を明示する。

## 検証と後続

[MCP-002 の検証](../testing/mcp-002.md) に受け入れ条件、Rust tests と実 client の対応を記録する。official Python SDK は `mcp==1.26.0`（`LATEST_PROTOCOL_VERSION=2025-11-25`）に固定し、scratch venv だけに導入する。product dependency に追加しない。

GPU / hardware codec、full workspace host run、Linux / Windows runtime、実 TLS terminator、最新 protocol、OAuth、多ユーザー権限、subscription / replay、MCP tasks と sampling は未検証または未対応。受け入れ判定、backlog / ADR index の更新と commit は supervisor が担当する。
