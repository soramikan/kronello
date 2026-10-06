# ADR-0084 MCP 2026-07-28 の要求ごとの protocol 契約

- 状態: 採用
- 日付: 2026-10-06
- 対象: MCP-003
- 部分置換: ADR-0064 の protocol version / transport session 範囲。共有 service・明示 project path・security / limit policy は維持する。

## 決定

`2026-07-28` と legacy `2025-11-25` / `2025-06-18` を同じ endpoint / stdio process で扱う。
modern request は `_meta.io.modelcontextprotocol/protocolVersion` と
`_meta.io.modelcontextprotocol/clientCapabilities` を毎回検査し、過去の request の
capabilities を引き継がない。modern request に initialize handshake は要求しない。
legacy initialize の negotiated version / initialized lifecycle は独立して維持する。

`server/discover` は実際の supportedVersions、tools / resources / prompts を返す。
modern の結果は `resultType: complete` と serverInfo metadata を付ける。
CacheableResult の ttlMs は 0、cacheScope は private とし、project 内容を権限境界を越えて
cache できると広告しない。sampling / elicitation / roots / subscriptions / tasks は実装せず広告しない。

modern HTTP は session ID と GET / DELETE を使わず、POST ごとに独立した dispatch を作る。
MCP-Protocol-Version / Mcp-Method / tools, resources, prompts の Mcp-Name を body と照合する。
Mcp-Name の Base64 sentinel を UTF-8 decode して比較する。現行 tool schema は
x-mcp-header を広告せず、未認識の Mcp-Param は無視する。missing / malformed / mismatch は
HTTP 400 / HeaderMismatch (-32020)、未知 version は HTTP 400 /
UnsupportedProtocolVersionError (-32022)、未知 method は HTTP 404 / -32601。
legacy の HTTP session / DELETE と authentication / Origin / Host 検証は維持する。

modern HTTP の SSE response stream の drop はその request の cooperative cancellation とする。
legacy stream の drop は従来の明示 cancellation notification / session close に委ねる。
どちらも detached render job の終了と同一視しない。

## 一次仕様

2026-10-06 に以下の全文と schema を照合した。

- [Versioning](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
- [Discovery](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)
- [stdio](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)
- [Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
- [2026-07-28 schema.ts](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2026-07-28/schema.ts)

[公式 Python SDK](https://github.com/modelcontextprotocol/python-sdk) の最新公開版 2.3.0 /
mcp-types 2.3.0 の Client で実 stdio / HTTP の同等性を検証する。テスト依存の固定値は
scripts/mcp-003-client-requirements.txt に記録する。product の Rust runtime 依存ではない。
