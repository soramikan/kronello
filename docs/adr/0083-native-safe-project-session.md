# ADR-0083 native セッション中の安全モード排他

- 状態: 採用
- 日付: 2026-10-06
- 対象: FFI-002
- 部分置換: ADR-0056 の各 request で store を開閉する範囲。非同期 FIFO・明示 project path・worker 契約は維持する。

## 決定

FFI worker は `ProjectSession` を持ち、安全モードの `ProjectStore` を session の生存中保持する。
同じ canonical project path の Command / Query は service の `StoreLease` を通して
同じ store を一時的に借り、request の終了・エラー・unwind で session に返す。
入口専用の document / revision は作らない。通常モードは従来の request 単位の open / close を維持し、
外部編集と WAL の更新通知を継続する。

thread-local の routing scope は FFI worker の request / preview / subscription の処理に限る。
純粋 model と store の unsafe forbid は維持する。`kronello_close` は queued work が終わった後に
worker の store を drop する。プロセス終了時には OS が file lock を解放する。

初回 open が `PROJECT_LOCKED` などで失敗した session は同じ typed error を保持し、他の holder が
終了しても後続 request で transient store を開かない。新しい session を明示的に open する。
初回 `PROJECT_NOT_FOUND` は既存の project.create flow のため許容し、作成後の次 request で
`ProjectSession` を採用する。

音声の binary producer は既存の明示 project path から native session を特定し、
worker FIFO に snapshot capture を送る。共有 service の `capture_audio_input` が同じ store から
immutable `AudioPreparationInput` を生成して channel で渡し、producer が compile / decode する。
新しい作品状態を持たず、safe store を他の thread から読み直さない。
revision fence は capture と prepare の両方で確認する。

## 検証

実 native cdylib をロードした独立プロセス、実 CLI / MCP を使い、安全モード中の排他、
共有編集、非同期 close、生存中の失敗 session、強制終了後の解放を検証する。
同期 folder の自動判定 fixture と本物の cloud 同期・network filesystem は区別する。
