# ADR-0020: バックログの正本をリポジトリ内のファイルとする

- 状態: 採用
- 日付: 2026-10-02

## 背景

実装の多くをコーディングエージェントが担う。タスクの依存関係と受け入れ条件を、エージェントがオフラインで読み、機械的に検証できる必要がある。

## 決定

- `docs/backlog/backlog.json` をバックログの正本とする。
- `docs/backlog/BACKLOG.md` は `scripts/backlog.py render` で生成し、直接編集しない。
- 依存の存在・循環・マイルストーン順序は `scripts/backlog.py check` で検証する。

## 影響

- タスクの変更は設計文書と同じコミットで追跡できる。
- GitHub Issues とは同期しない。Issue を使う場合も正本はファイルとする。

## 関連

- [バックログ](../backlog/README.md)
