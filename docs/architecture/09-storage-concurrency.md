# 09 保存と同時編集

v0.2 仕様の §3.3 を具体化した章。関連: [ADR-0006](../adr/0006-document-vs-render-cache.md)、[ADR-0011](../adr/0011-local-sqlite-source-of-truth.md)、[ADR-0016](../adr/0016-single-file-project.md)、[ADR-0017](../adr/0017-multi-process-optimistic-concurrency.md)、[ADR-0026](../adr/0026-selective-undo.md)〜[ADR-0030](../adr/0030-history-retention.md)。

## 保存形態

| 対象 | 置き場所 | 性質 |
|---|---|---|
| 作品データ | `<name>.kronello`（単一の SQLite ファイル） | 正本 |
| 素材 | プロジェクト外。locator + content hash で参照 | 不変の外部ファイル |
| レンダーキャッシュ | OS のキャッシュ領域 | 削除してよい |
| ジョブの記録と固定スナップショット | ユーザーごとの状態領域（[14 ジョブ](14-jobs.md)） | このマシンでの実行記録 |
| GUI の UI 状態 | ユーザーごとの状態領域。プロジェクト ID で紐付け（[10 デスクトップ GUI](10-desktop-gui.md)） | 作品から分離 |
| 受け渡し用フォルダ | `project.collect` で書き出す | プロジェクトの複製と素材を相対パスでまとめたもの |

キャッシュの削除で作品データを失わない。素材の content hash が一致しない場合は検証で報告する。

## `.kronello` の中身

SQLite の現在状態、イベント、逆操作情報、リビジョンを一つのトランザクションで更新する。

- **現在状態**: 文書モデルの最新の姿。
- **イベント**: 適用されたコマンドの記録（監査、差分、外部変更の検知に使う）。発行した session と、変更したキーの集合を含む。
- **逆操作情報**: Undo のための情報。イベントごとに保存する。
- **revision**: 変更トランザクションごとに進む版番号。
- **完全スナップショット**: バージョン付き。イベントだけを唯一の保存形式にすると過去コマンドの意味変更が問題になるため保持する。
- **idempotency の記録**: `idempotency_key` と適用結果。

JSON は不変スナップショットまたはインポート形式であり、SQLite と並行して書き換える第二の正本にしない。JSON の形式は「JSON スナップショット」を参照。

## 版と移行

`schema_version` は構造、`semantic_version` は補間・合成などの意味を表す（[01 データモデル](01-data-model.md)）。
migration が失敗した場合に元データを壊さない。未知の機能は保存時に失わない。

[ADR-0045](../adr/0045-snapshot-compatibility-boundaries.md) に従い、構造と意味を別々に検証し、対応する外枠の未知内容は opaque に保持する。lossless な保持が保証できない構造版は原本を変更せず拒否する。構造を読めても、未知の意味に依存する変更・最終レンダーは許さない。

migration は原本を保全し、検証成功後に原子的に確定する。意味だけの変更も意味の版を管理し、過去の固定ジョブ入力を上書きしない。旧イベントを新しいコマンド意味で無条件に再実行しない。公開 `schema_version`、SQLite 内部版、アプリ版、revision は別の境界とする。

## 複数プロセスからの同時編集

GUI・CLI・MCP サーバーはそれぞれ別プロセスとして同じ `.kronello` を開ける。エージェントが CLI / MCP で加えた変更を、開いている GUI がそのまま確認できることを目的とする。

### 書き込み

1. 書き込みトランザクションを排他で開始する（他プロセスの書き込みを待たせる）。
2. `idempotency_key` を確認する。同じキー・同じ canonical payload なら保存済みの結果を返し、異なる payload なら拒否する（SERVICE-001 実装済み）。
3. 新しい適用の場合は現在の revision を読み、要求の `base_revision` と照合する。一致しなければ競合として拒否する。
4. 現在状態を更新し、イベントと逆操作情報を追加し、revision を進める。
5. commit する。

プロセス内の Project Service が自プロセスの書き込みを直列化し、プロセス間の直列化は SQLite の書き込みロックが担う。「単一 writer」とは、任意の瞬間に書き込みトランザクションが一つであることを指す。

### 外部変更の検知

各プロセスは自分が保持するスナップショットの revision を知っている。他プロセスが commit したことを検知したら、自分の revision 以降のイベントを読んでスナップショットを更新する。GUI は検知後に再読込し、表示を更新する。

検知できなかった場合でも安全側に倒れる。古い `base_revision` による書き込みは必ず拒否されるため、古い状態の上に黙って上書きすることはない。

### 競合時の扱い

- 計画（plan）は古くなったら適用しない。呼び出し側が最新の状態を読み直して計画を作り直す。
- GUI のドラッグ操作など連続する変更は、確定時に一つのコマンドとして発行する。確定前に外部変更が来た場合の UI 上の扱いは GUI-001 で設計する。

## Undo

[ADR-0026](../adr/0026-selective-undo.md) による。

### 意味

- Undo は「対象イベントの逆操作を、新しいコマンドとして発行する」ことである。revision は常に前進し、履歴を巻き戻さない。
- 一つの transaction（plan）が一つのイベントであり、Undo の単位になる。
- Undo 自体もイベントとして記録され、どのイベントを取り消したかを持つ。Redo は、その Undo イベントを取り消すことである。

### 変更したキー

各イベントは、変更したキーの集合を記録する。

| 変更の種類 | キー |
|---|---|
| 値の変更（Property の主値源、キーフレーム、Modifier、公開入力など） | `(オブジェクト ID, Property ID)` |
| 構造の変更（ノードの追加・削除・親の変更・順序変更、クリップの配置など） | 対象オブジェクトの ID と、その親コンテナの ID |

### 競合

対象イベントより後にあり、まだ取り消されていないイベントが、対象イベントと同じキーに触れている場合、Undo を `UNDO_CONFLICT` で拒否する。

- 拒否したときは何も適用しない。部分的な適用はしない。
- エラーには、競合したイベントの ID とキーを含める。
- 誰が発行したイベントかは判定に使わない。自分の後続の操作であっても、先にそれを取り消す必要がある。
- 式やレイアウトを経由した間接的な影響は競合とみなさない。Undo を適用した結果の状態は通常の検証を通り、循環などで無効になる場合は拒否する。

### 範囲

| 入口 | 取り消せる範囲 |
|---|---|
| GUI の Undo（Cmd+Z） | その GUI セッション（プロジェクトを開いてから閉じるまで）が発行した、未取り消しのイベントを新しい順に |
| CLI / MCP、GUI の履歴一覧 | `history.list` で得た event ID を指定した任意のイベント |

逆操作情報とイベントは `.kronello` に永続化するため、GUI を再起動した後や別プロセスからも、event ID を指定すれば同じ規則で取り消せる。別プロセス（エージェント）が加えた変更を GUI の Cmd+Z が取り消すことはない。

## 履歴の保持

[ADR-0030](../adr/0030-history-retention.md) による。

- 履歴（イベントと逆操作情報）を自動では削除しない。
- `history.compact` は、指定した revision より前の履歴を切り捨て、その時点の完全スナップショットを基点として残す。
- 切り捨てた範囲のイベントは Undo できない。
- 履歴が大きくなった場合は `project.validate` が警告する。STORE-001 の `history_size()` は、イベントの patch / inverse / changed keys と、現在以外の完全 snapshot の UTF-8 payload 合計が **256 MiB 以上**なら `warning=true` を返す。検証応答への反映は `project.validate` の service 公開時に行う（SERVICE-001 の edit / history 操作には含めない）。SQLite の空きページや index、現在文書、idempotency receipt はこの論理量に含めない。
- STORE-002 以降は初期 revision 0、`revision % 64 == 0`、`compact(r)` の基点だけに完全 snapshot を保存する。サイズによる追加保存は STORE-003 に委ねる。任意 revision は直前の完全 snapshot と連続したイベント patch を最大 63 個再適用して復元する。
- `compact(r)` は同じ書き込み transaction 内で `r` を復元して完全 snapshot を確保し、`< r` を削除する。`r` のイベントと現在文書・revision は変えない。idempotency receipt は compact 後も残り、キーの再適用を防ぐ。失敗時は基点の追加を含め rollback する。DB ファイルの物理縮小は保証しない。

## ジャーナル方式

[ADR-0027](../adr/0027-wal-single-file-on-close.md) による。

- 通常は WAL モードで開く。GUI が読みながら、別プロセスが書ける。
- 開いている間は付随ファイル（`-wal`、`-shm`）ができる。最後のプロセスが閉じるときに checkpoint し、付随ファイルを残さない。
- 異常終了で付随ファイルが残った場合は、次に開いたときに回復する。
- 開いている最中のファイルを直接コピーすることはサポートしない。複製には `project.export` や `project.collect` を使う。

### 安全モード

ネットワークファイルシステムやクラウド同期フォルダ上と判定した場合は、警告を出して安全モードで開く。

- 非 WAL で、開けるプロセスは一つだけ。
- 安全モード中に他のプロセスが開こうとすると `PROJECT_LOCKED` を返す。
- したがって、同期フォルダ上ではエージェントとの同時編集はできない。

判定と上書き手段は [ADR-0046](../adr/0046-store-format-and-location-policy.md) で具体化した。

`OpenMode::Auto` は canonical path と実ファイルシステムを調べる。macOS の home 配下 `Library/CloudStorage` / `Library/Mobile Documents`、Dropbox / OneDrive / Google Drive 系フォルダ名、および `statfs` の smbfs / nfs / afpfs / webdav 等を安全モードにする。Linux は NFS / SMB / CIFS / SMB2 / FUSE、Windows は UNC を検出する。`ForceNormal` / `ForceSafe` で誤判定を上書きできるが、既存プロセスの排他ロックは突破しない。判定器は `LocationDetector` として注入できる。非標準同期先や Windows のドライブ文字でのネットワーク接続は完全には検出できず、`ForceSafe` を指定する。

安全モードは DELETE journal と SQLite exclusive locking を併用する。同一 OS 内では一時領域の小さな shared / exclusive lock ファイルでモードを調停する。これは作品の正本・キャッシュではなく、プロジェクトの外に置く。プロジェクト本体への追加 whole-file lock は macOS で SQLite と干渉するため使わない。ロックの identity は Unix では device / inode、それ以外は canonical path に基づく。二重ロックを避けるため終了時に unlink しない。close / Drop では SQLite connection を閉じた後に明示的に unlock する。open の失敗時と初期化用ロックの終了時にも unlock する。descriptor の close だけでは、並行する子プロセス生成で継承された descriptor が exec までロックを延命しうる。プロセス終了でも OS がロックを解放する。同じ領域の一時的な exclusive open lock で、新規 DB の初期化と journal mode の切り替えも直列化する。

## 素材の参照

[ADR-0028](../adr/0028-asset-references-and-relink.md) による。

- Asset の locator は、プロジェクトファイルの場所を基準にした相対パスと、絶対パスの両方を持つ。
- 解決は相対パス、絶対パスの順に試し、見つかったファイルの content hash を照合する。
- 見つからなければ `ASSET_MISSING`、hash が一致しなければ `ASSET_HASH_MISMATCH` として報告する。別のファイルへ自動で差し替えない。
- `asset.relink` は、指定したフォルダから hash が一致するファイルを探してパスを更新する。
- 内容の違うファイルへの差し替えは `asset.replace` で明示的に行う。
- `project.collect` は、プロジェクトの複製と素材を相対パスでまとめたフォルダを書き出す。別のマシンへ渡すときに使う。

hash 照合の頻度と高速化は MEDIA-001 で設計する。

## JSON スナップショット

[ADR-0029](../adr/0029-public-json-schema.md) による。

- 文書モデルの JSON 表現を、版付きの公開スキーマとして一つ定義する。Rust の文書モデル型から JSON Schema を生成し、リポジトリで管理する。
- ジョブの固定スナップショット、`project.export`、`project.import`、Command / Query API の payload、FFI の payload は同じ型定義を共有する。
- `schema_version` を持ち、未知のフィールドを保持する。有理数は 10 進文字列で表す。
- JSON は書き出した時点の不変の写しであり、`.kronello` と並行して編集する正本ではない。

RenderSnapshot は同じ公開構造版と文書の意味の版を含む `semantic_versions` を持ち、資産・フォント・データの lock と profile を固定する（ADR-0045「RenderSnapshot」）。未知内容の round-trip は値・型・所属・参照の保持であり、JSON の空白・キー順の byte 一致を要求しない。安全に保持できない import / export を成功扱いにしない。

## STORE-001 / STORE-002 の実装境界

`crates/kronello-store` は SQLite 保存層を実装した。ここで記載した `project.export/import`、`history.compact` に相当する Rust の保存 API は存在するが、Service / CLI の project.create / import / export / info は CLI-001、plan / apply / undo と最小 history.list は SERVICE-001 で実装した。MCP と history.compact の transport は後続タスクである。公開型・判定方針は [ADR-0046](../adr/0046-store-format-and-location-policy.md)、受け入れ条件の検証は [STORE-001 の検証](../testing/store-001.md) を参照する。

| テーブル | 内容 |
|---|---|
| `project` | singleton の現在文書 JSON と revision |
| `events` | revision / UUID event ID / session / mutations / inverse / changed keys / optional idempotency key / optional undo_of |
| `snapshots` | 初期・64 revision ごと・compact 基点の完全文書 JSON。旧ファイルの全 revision snapshot も保持・読込する。公開構造版と文書意味版を内包 |
| `idempotency` | unique key / payload / event ID / revision / 完全な適用結果（Event）。SERVICE-001 の canonical service_payload を payload 内に保持し、同じ key / payload の結果を復元 |

`apply` は `BEGIN IMMEDIATE` 後に revision を照合し、文書・event・逆操作・該当 revision の完全 snapshot・receipt を同じ transaction で更新する。patch はオブジェクトメンバーの経路に対する `Set` / `Remove`（root の `Set` は完全文書の差し替え）。SERVICE-001 の patch は ID を持つ配列 member を UUID で選択する。描画順等の scalar 配列は集合ごと置き換え、配列位置を ID に使わない。逆操作は保存層が元値から自動生成する。変更キーの算出と通常のモデル意味検証は呼び出し側の責務とし、import / restore は対象集合のキーを保守的に列挙する。低水準 `apply` の既存キーは `IDEMPOTENCY_KEY_EXISTS` として拒否する。SERVICE-001 は下記 `apply_with_payload` で同一 payload の元 Event を復元し、service の selective undo が保存済み inverse / changed keys / undo linkage から競合を判定する。

`kronello-model::Project` の初期版は schema / semantic version 1、UUID id、name、Composition / Curve 集合と未知フィールドを持つ。`DocumentObject<T>` の既知型で decode できなければ全 object を opaque に保持し、未知ノード・enum・入れ子フィールドも失わない。通常変更は未知の意味や opaque 内容があると `UNSUPPORTED_FEATURE`。import / export / snapshot 復元は保持を許す。[公開 JSON Schema](../../schemas/project-v1.schema.json) は型から生成し、rational の num / den は 10 進文字列。未知内容を含む保存外枠に適合することと、その内容を実行できることは区別する。

内部版は `PRAGMA user_version=1` で、公開構造版・意味版・revision とは別。認識できない DB / 構造版は拒否する。`migrate_schema` は信頼された Rust コード専用の transaction hook で、SQL を Command payload として公開しない。失敗は DDL・データ・内部版を rollback する。旧版の具体的な migration 経路はまだなく、新規 DB の初期化のみ実装した。

`snapshot_at(r)` は一つの読み取り transaction で直前の完全 snapshot と `r` までの連続 patch を読む。他 connection の apply / compact と混ざった状態を読まない。patch ごとに文書を decode・構造検証し、通常保存と同じ正規化を保つ。必要な patch の欠落・破損・適用不能・文書不適合や周期 snapshot の欠落による 64 個以上の再適用は、revision を持つ `HISTORY_REPLAY_FAILED`。保持範囲外は `SNAPSHOT_NOT_FOUND`。`restore_snapshot` はこうして復元した文書を新 revision として保存し、Command の意味や逆操作を再評価しない。既存の全 revision snapshot は削除・変換せず優先して使う。保存 schema と `user_version=1` の変更・migration は不要。[STORE-002 の検証](../testing/store-002.md) を参照する。

`render_cache_location` は OS cache dir の `render` namespace を返し、プロジェクトの親領域との重なりを拒否する。`.kronello` に cache table はない。文書の `content_hash` は全未知内容・schema / semantic version を含むキー順整列済み JSON を UTF-8 compact に serialize した SHA-256。`float_roundtrip` と `arbitrary_precision` により未知 JSON の大きな整数・数値の綴りも保持する。数値表記の違いは別 hash になりうる。RenderSnapshot の lock / profile を含む identity は後続の実装で組み合わせる。

SQLite は bundled の `rusqlite` を使用する。`rusqlite` / `libsqlite3-sys` の配布 crate は MIT、同梱 SQLite 本体は [public domain](https://www.sqlite.org/copyright.html) で、GPL 構成の FFmpeg を新たに取り込まない。

## SERVICE-001 の保存層追加

`ProjectStore::apply_with_payload` は service の canonical payload を receipt に保存する。`BEGIN IMMEDIATE` 内で receipt を先に照合し、一致すれば元の Event、異なれば `IDEMPOTENCY_KEY_REUSED`。新規要求だけ revision を照合して通常の atomic apply を行う。既存 `apply` の生 patch caller の契約は変えない。`snapshot_and_events` は文書と履歴を一つの読み取り transaction で取得する。

保存 `Set` / `Remove` の path は、object member のほか ID を持つ配列 member を UUID で選択できる。配列番号は使わない。追加・削除・逆操作が別 Property の後続変更を上書きしない。空の optional shapes / texts は公開 serializer が省略するため、member Set で collection を作り、inverse は member Remove とする。所有順序など scalar 配列は従来どおり全体を置換し、service が親コンテナの競合を判定する。64 revision ごとの完全 snapshot、既存 patch の読み込み、compact の receipt 保持は従来どおり。詳細は [08 API](08-api-cli-mcp.md#m2-service-001-の実装範囲) と [検証](../testing/service-001.md) を参照する。
