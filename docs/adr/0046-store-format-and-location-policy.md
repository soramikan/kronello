# ADR-0046: 保存の外枠・安全モード判定・履歴警告を固定する

- 状態: 採用
- 日付: 2026-10-03
- 対象: STORE-001

## 背景

ADR-0027 / ADR-0030 / ADR-0045 が STORE-001 に委ねた、保存場所の判定・利用者による上書き・履歴の警告閾値・版の初期値・content identity の符号化を具体化する。既存 ADR の方針は置換しない。

## 決定

- 公開 `schema_version=1`、文書 `semantic_version=1`、SQLite `user_version=1` をそれぞれ独立した境界として開始する。Project は UUID の `id`、`name`、Composition 集合、Curve 集合を持つ。
- Rust 型から `schemas/project-v1.schema.json` を生成する。既知の Composition / AnimationCurve のほか、同じ UUID `id` を持つ opaque object を許す。既知型として厳密に decode できないオブジェクトは、入れ子を含む全体の JSON を保持する。Schema の適合は保存外枠の適合であり、opaque 内容の実行能力を意味しない。
- 未知の意味版・未知フィールド・opaque object を含む文書は、import / export / 保存・完全 snapshot の復元を許す。通常の変更は読み取り専用として拒否する。既知部分の独立性を証明する細分化は SERVICE-001 以降で行う。
- 通常は WAL + `synchronous=FULL`。安全モードは DELETE journal + SQLite `locking_mode=EXCLUSIVE`。同一 OS のモード競合は OS 一時領域の `kronello-project-locks/<identity>.lock` の shared / exclusive lock でも調停する。macOS のプロジェクト本体への追加 whole-file lock は SQLite と干渉するため使わない。
- ロックの identity は Unix で device / inode、それ以外は canonical path の SHA-256。ロックファイルは作品データを持たず、プロジェクト外に置く。終了時に unlink すると別 inode の二重ロックが起きるため削除しない。プロセス終了・異常終了で OS がロックを解放し、一時領域の掃除は OS に任せる。初期化と journal mode 変更中だけ、同じ identity の別の open lock を exclusive に取得して競合を避ける。ネットワーク上の異なるホスト間には SQLite の exclusive lock を用いる。ファイルシステム自身がロックを正しく提供しない場合の安全性までは保証しない。
- `Auto` は canonical path とファイルシステムを判定する。macOS は `statfs` の種類（smbfs / nfs / afpfs / webdav 等）と、home 配下の `Library/CloudStorage`、`Library/Mobile Documents`、Dropbox / OneDrive / Google Drive 系のフォルダ名を使う。Linux は NFS / SMB / CIFS / SMB2 の magic と FUSE を安全側に判定する。Windows は UNC を検出する。
- 誤検出には `ForceNormal` / `ForceSafe` を指定できる。強制指定でも、既に開いている相反するモードのロックは突破できない。`LocationDetector` を注入可能にする。非標準同期フォルダ、Windows のネットワークドライブ文字、別名の同期サービスは自動検出の限界であり `ForceSafe` を使う。
- 履歴の警告閾値は **256 MiB 以上**。イベントの patch / inverse / changed keys と、現在 revision 以外の完全 snapshot の UTF-8 payload 合計を測る。SQLite の空きページ、index、現在文書、idempotency receipt はこの論理量に含めない。自動削除しない。
- M1 は全 revision の完全 snapshot を保存する。`compact(r)` は revision `< r` のイベント・snapshot を transaction 内で削除し、`r` の完全 snapshot とイベントを残す。idempotency receipt は再適用防止のため残す。ファイルの物理縮小を保証する API ではない。
- 文書の content hash は全未知内容・構造版・意味版を含む JSON 値をキー順で整列し、UTF-8 compact JSON の SHA-256 とする。`serde_json` の `float_roundtrip` と `arbitrary_precision` を維持し、未知 JSON の大きな整数・数値の綴りも失わない。数値表記の異なる未知 JSON は別 identity になりうる。ジョブ用 RenderSnapshot の lock / profile と複合する identity は後続のジョブ実装の対象であり、文書 hash のみでレンダーキャッシュを同一視しない。

## 影響と範囲

`kronello-store` は同期的な保存 API と transactional patch を実装する。Command / Query API、idempotency の同一 payload 判定と再送結果、selective undo / `UNDO_CONFLICT`、`project.validate` の警告応答、最終レンダーの能力判定は SERVICE-001 等の後続タスクである。STORE-001 はそのための session / changed keys / inverse / undo linkage / receipt を保存する。

実ネットワークマウントと各クラウド同期クライアント、Windows / Linux の実プロセスでの振る舞いは未検証。注入した種類・パスで検出方針を検証し、macOS ローカル環境ではプロセス競合と強制終了復旧を検証する。

## 関連

- [09 保存と同時編集](../architecture/09-storage-concurrency.md)
- [STORE-001 の検証](../testing/store-001.md)
