# バックログ

実装計画のタスク一覧。未実装の計画データであり、GitHub 等に登録した Issue ではない（[ADR-0020](../adr/0020-backlog-in-repository.md)）。

| ファイル | 役割 |
|---|---|
| [backlog.json](backlog.json) | 正本。機械可読。編集するのはこのファイルだけ |
| [BACKLOG.md](BACKLOG.md) | `backlog.json` から生成した一覧。直接編集しない |

## 使い方

```bash
python3 scripts/backlog.py check
```

```bash
python3 scripts/backlog.py render
```

`check` は次を検証する。`render` は検証に通った場合だけ `BACKLOG.md` を再生成する。

- 必須フィールド、ID の重複、`task_count` の一致
- マイルストーン・優先度・状態の値
- 依存先が存在すること、依存の循環がないこと
- 依存先が自分より後のマイルストーンにないこと
- `done` のタスクの依存先がすべて `done` であること

## フィールド

| フィールド | 値 |
|---|---|
| `id` | `<領域>-<3桁>`。一度付けた ID は変えない |
| `milestone` | `M0`〜`M6`（[milestones.md](../roadmap/milestones.md)） |
| `priority` | `P0`（そのマイルストーンに必須）/ `P1` / `P2` |
| `area` | 領域 |
| `status` | `planned` / `in_progress` / `done` / `dropped` |
| `dependencies` | 先に完了している必要があるタスクの ID |
| `acceptance_criteria` | 完了の判定条件。すべて確認できて初めて `done` |

## 0.4 からの変更（schema_version 0.5）

ADR-0027〜0041 の決定を反映した。50 → 51 タスク。

- 追加: CI-001（M0、ツールチェーン固定と CI）。
- STORE-001: WAL と安全モード、公開 JSON スキーマの export / import、`history.compact`。
- MEDIA-001: 素材の解決と再リンク、ソフトウェアエンコーダー、同梱 FFmpeg の manifest。
- FFI-001 / GUI-001: C ABI + JSON、UI 状態の保存先。
- JOB-001: ジョブディレクトリの掃除。
- QA-001: テスト素材の方針。
- EXPR-001: AST を正本とする構造。
- COLOR-001: HDR の方針。

## 0.3 からの変更（schema_version 0.4）

色・ジョブ・Undo の決定（ADR-0024〜0026）を受け入れ条件へ反映した。タスク数は 50 のまま。

- ARC-001 / GPU-001: 作業用色空間と sRGB 色入力の規約。
- STORE-001 / SERVICE-001 / API-001 / GUI-001: イベントの session・変更キー・逆操作情報、`edit.undo` と `UNDO_CONFLICT`、`history.list`。
- JOB-001: 切り離した worker、状態 DB、同時 1 ジョブ、heartbeat。
- RECOVERY-001: `interrupted` ジョブの再開。
- COLOR-001: HDR の基準白・表示変換・色域マッピング。

## v0.2 からの変更（schema_version 0.3）

元のバックログは [archive](../archive/motion_editor_backlog_v0_2.json) に保管している。45 → 50 タスク。

追加:

| ID | 段階 | 理由 |
|---|---|---|
| AUDIO-000 | M2 | 基本音声（デコード・ミックス・音声付き書き出し）のタスクがなかった |
| FX-001 | M2 | 縦断テストが要求する shadow 等の基本エフェクトのタスクがなかった |
| FFI-001 | M3 | GUI を OS ネイティブ + 同一プロセス FFI としたため |
| AUDIO-002 | M3 | GUI でのリアルタイム再生と A/V 同期 |
| INTEGRATION-002 | M3 | 縦断テストを 2 段階に分割 |

変更:

- STORE-001: 単一 `.cinewright` ファイル、複数プロセスからの書き込みの revision 照合を受け入れ条件に追加。
- SERVICE-001: idempotency の記録をプロジェクト内に保存する条件を追加。
- TEMPLATE-001: 背景帯の単方向追従と overflow 検出を追加（縦断テスト第 1 段階に必要）。
- INTEGRATION-001: 第 1 段階（CLI / MCP）に限定。依存に AUDIO-000、FX-001 を追加。
- GUI-001: macOS ネイティブ GUI に変更。依存を FFI-001 に変更。外部変更の検知を追加。
- GPU-001 / GPU-003 / MEDIA-001: macOS 先行、LGPL 構成の FFmpeg を反映。
- AUDIO-001: 依存に AUDIO-000 を追加。
