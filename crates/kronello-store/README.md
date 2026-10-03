# kronello-store

STORE-001 の同期 SQLite 保存層。作品は単一の `.kronello`、レンダーキャッシュは `render_cache_location` が返す OS cache dir に分離する。

`ProjectStore::open` / `open_with_detector`、`snapshot` / `snapshot_at`、`apply`、`import_json` / `export_json`、`restore_snapshot`、`events_since`、`idempotency_record`、`compact`、`history_size`、`content_hash`、`close` を提供する。`ApplyRequest` は base revision、session、serializable な `Mutation` 集合、`ChangedKey` 集合、optional idempotency key / undo linkage を持つ。逆操作は保存層が生成する。

`Mutation::Set` / `Remove` の path は object member の String 列。配列は集合全体を置換する。サービス側で安定 ID から変更を計画し、changed keys と意味的妥当性を検証して渡す。SELECT 後に書くのではなく、`BEGIN IMMEDIATE` 内で revision を照合する。衝突は `StoreError::code()` の `REVISION_CONFLICT` 等で識別できる。

`OpenMode::Auto` / `ForceNormal` / `ForceSafe` を選べる。検出した sync / network の理由は `detected_location()`、実際の mode は `safe_mode()` で取得する。safe mode には SQLite exclusive lock とプロジェクト外の OS 一時領域のモード調停 lock を使う。mode override は既存 process の lock を突破しない。

未知フィールド・意味版・opaque object は保存 / export / restore で保持し、通常の変更は拒否する。`migrate_schema` は信頼された Rust 内部実装専用の transaction hook。任意 SQL を Command / Query payload として受け付ける API ではない。idempotency の payload 比較・成功再送応答と selective undo は SERVICE-001 で実装する。

[保存設計](../../docs/architecture/09-storage-concurrency.md)、[ADR-0046](../../docs/adr/0046-store-format-and-location-policy.md)、[受け入れ条件とテスト](../../docs/testing/store-001.md) を参照。
