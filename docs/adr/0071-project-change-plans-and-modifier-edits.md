# ADR-0071: Project 作成・import の計画と再送、Modifier の型付き編集

- 状態: 採用
- 日付: 2026-10-05
- 対象: SERVICE-002

## 背景

SERVICE-001 は通常編集の plan / apply / receipt / selective Undo を提供したが、
project.create / project.import は plan と再送結果を持たなかった。
Property.modifiers は保存できても、入口が共有する型付き編集 Command がなかった。
本 ADR は ADR-0001 / 0004 / 0010 / 0026 / 0029 / 0043 / 0045 / 0046 / 0058 を具体化し、置換しない。

## Project の変更計画

- 共有 Query `project.create_plan` / `project.import_plan` を追加する。
  前者は `{project,document}`、後者は `{project,base_revision,document}`。
  結果 `ProjectChangePlan` は operation（create / import）、正規化した project、
  expected_absent、base_revision、base_content_hash、candidate、plan_hash を持つ。
- 出力先の親ディレクトリを canonicalize し、末尾の filename を結合する。
  `.kronello` 拡張子と既存 local locator policy を照合する。create の target が既存なら
  `PROJECT_EXISTS`。dangling symlink も既存として扱い、上書きしない。
- plan は文書の保存検証と immutable template pin の既存検証を通す。
  import は一つの read-only SQLite snapshot で base_revision を照合する。
  保存できる未知内容は candidate に保持する。実行可能性を保証する計画ではない。
- plan_hash は plan_hash 自身を空文字にした計画全体を JSON object のキー順で整列し、
  UTF-8 compact JSON の SHA-256 とする。型 decode 後の候補、正規化 target、
  operation と import の元文書 hash / revision を含む。配列順は保持する。
- 既存 project.create / project.import に optional `plan_hash` と `idempotency_key` を追加する。
  指定した hash を照合できない場合は `PLAN_HASH_MISMATCH`。import は writer transaction
  内で照合し、古い base_revision は `REVISION_CONFLICT`。同 revision の別文書への
  ファイル置換も base_content_hash によって検出する。黙って別の計画を適用しない。
  両 field を省略した従来の direct Command は引き続き利用できる。

## 作成の出力予約と durable receipt

- SERVICE-002 の「出力予約」は apply 時の no-clobber publication とする。
  plan 時は expected_absent を返すだけで target や予約 token を作らない。
  plan 後に別の writer が作った target を採用・上書きしない。
- 出力先と同じ親に staging SQLite ファイルを作り、初期 import と create の receipt を
  同じ transaction で保存する。close による checkpoint、staging の sync 後に
  `persist_noclobber` で初期化済みファイルだけを公開する。
  失敗時は RAII で未公開 staging を削除する。対象 pathname に未初期化ファイルを置かない。
  別 key の同時 create は一つだけが成功し、敗者は `PROJECT_EXISTS`。
- 既存 target に一致する create receipt がなければ `PROJECT_EXISTS`。
  既存 receipt の key が同じでも canonical payload が違えば `IDEMPOTENCY_KEY_REUSED`。
  同 key / payload の create が競争した場合は公開済み receipt を再生する。
- import は既存 idempotency table を利用し、receipt 照合を revision より先に行う。
  元文書の検証、全置換 patch / inverse / service 導出 keys、revision と receipt を
  `BEGIN IMMEDIATE` の一つの transaction で保存する。
- canonical payload は operation、正規化 project、文書、optional plan_hash と、
  import の unsigned decimal に正規化した base_revision。idempotency key 自身は lookup key。
  key の scope は対象 .kronello の idempotency table。同じ作品内の edit / undo 等とも
  共有され、異なる operation で使い回せない。別作品は独立した key 空間を持つ。
- receipt は元の `ProjectInfo` 全体（open_mode、revision、content_hash を含む）を記録する。
  後続 edit / import / compact の後でも最新 info ではなく元の結果を返す。
  再送は READ_ONLY / query_only 接続で receipt を読み、作品本体の bytes / revision を変更しない。
  SQLite の WAL / SHM sidecar が作られることはありうる。
  既存 SQLite user_version / 公開 Project schema は変更しない。
- plan 時の persistent token は採用しない。孤立予約の回収・期限・所有者を新しく定義する
  lifecycle が不要で、計画を read-only に保てるためである。
  process の通常失敗時の cleanup と保存後の再送を検証対象とし、電源断耐久性・
  強制 kill 後の staging 回収・実ネットワーク FS の保証は今回検証していない。

## Modifier の編集と実行境界

- `EditCommand` に `modifier_insert {object,property,modifier,index}`、
  `modifier_replace {object,property,modifier}`、`modifier_remove {object,property,modifier}`、
  `modifier_reorder {object,property,order}` を追加する。object は既存の Node / Composition /
  Clip の UUID、property は PropertyId。replace は既存 ModifierId の完全な型付き置換で、
  enabled / key / version / parameters の変更も表す。ID の変更は remove + insert。
- insert の index は `[0,len]`、replace / remove は既存 ID が必要。
  reorder は ID の完全な permutation に限る。重複 ID、version=0、許されない descriptor、
  不正対象は `INVALID_EDIT`。Command の未知 field は `INVALID_REQUEST`。
- modifiers は順序付き配列全体の patch / inverse とし、変更キーは
  `Value(object_id,property_id)`。主値源変更と同じ Property は競合し、別 Property の
  selective Undo は保持する。active な Undo event も既存 ADR-0026 の規則で競合する。
  Undo / Redo の候補は通常の service 検証を通す。
- create / import は既存 validate_storage / validate_stored_project の保存検証を行い、
  実行可能性を検証しない。未知 object の opaque 保持と通常編集拒否の既存方針を維持する。
  型付き Modifier 編集は descriptor の capability / source 型、Modifier の構造、候補の
  静的 DAG を検証する。未実装の kind / version / parameters は保存データとして保持できる。
  Modifier algorithm の実装・対応広告は追加しない。
- 必要な Property に enabled な未実装 Modifier があれば sample / 最終 render は既存
  `UNSUPPORTED_FEATURE`。定数・curve・Expression の値で代替しない。
  disabled な Modifier は明示的に実行対象外とするが、descriptor の最終値検証は維持する。
- Expression は EXPR-001 の expression_set / property_source_set、正規 AST、予算と純粋
  DependencyGraph をそのまま使う。同じ plan に式設定と Modifier 編集を含められる。
  Expression の直接消費 Property と Modifier 編集は同じ Value key で競合する。
  人間向け構文（OQ-17）と新しい Modifier algorithm を決めない。
- 入口は Command の payload だけを受け取る。raw patch / mutations / inverse / changed_keys
  は受け付けない。CLI の一要求・一 stdout JSON と MCP の registry discovery を共有する。

## 検証

[service-002.md](../testing/service-002.md) に実 CLI の別 process / 同時 create / 同時 import、
receipt、配列順・inverse・selective Undo、Expression / unsupported 境界と検証環境を記録する。
