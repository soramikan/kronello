# ADR-0029: 版付きの公開 JSON スキーマを一つ定義する

- 状態: 採用
- 日付: 2026-10-02

## 背景

JSON は不変スナップショットまたはインポート形式と位置づけた（ADR-0011）が、スキーマは未定義だった。ジョブの固定入力（ADR-0025）、エクスポート / インポート、Command API の payload、FFI の payload がそれぞれ別の形式を持つと、入口ごとに意味がずれる。

## 決定

- 文書モデルの JSON 表現を、版付きの公開スキーマとして一つ定義する。
- Rust の文書モデル型から JSON Schema を生成し、リポジトリで管理する。
- ジョブの固定スナップショット、`project.export`、`project.import`、Command / Query API の payload は同じ型定義を共有する。
- `schema_version` を持ち、未知のフィールドを保持する。有理数は 10 進文字列で表す。

## 影響

- エージェントがプロジェクト全体を JSON として読める。
- ネイティブ GUI 側の型もこのスキーマから生成できる（ADR-0031）。
- スキーマは公開の互換性対象になり、変更には版管理と migration が必要になる。
- 検討した代替案: ジョブ用は内部形式とし公開 JSON は後回し、SQLite ファイルの複製をスナップショットとする。

## 関連

- [09 保存と同時編集](../architecture/09-storage-concurrency.md)
- [08 API・CLI・MCP](../architecture/08-api-cli-mcp.md)
