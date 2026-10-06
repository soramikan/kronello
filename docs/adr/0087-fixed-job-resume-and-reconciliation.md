# ADR-0087 固定入力ジョブの再実行と公開境界の照合

- 状態: 採用
- 日付: 2026-10-06
- 対象: RECOVERY-001
- 部分置換: ADR-0050 の interrupted 後の再開・temporary output 回収・rename 後の照合。独立 worker、固定 snapshot、FIFO slot、NOREPLACE 公開は維持する。

## 決定

共有 Command API に `job.resume` を追加する。active job と生存 worker の lease を奪わない。
interrupted / failed / canceled の固定 envelope を byte SHA-256、構造版、engine 版、
snapshot の意味版、必要機能、全 asset hash、font lock、保存済み request と job record の
identity で再検証する。現在の Project を読み直して入力を作り替えない。
pruned input は `JOB_INPUT_UNAVAILABLE` とする。

再開は file append や frame ごとの続き書き込みをしない。完了済み frame も含めて新しい
attempt の staging に最初から再実行し、`completed_frames` を 0 に戻す。
attempt は state DB の単一 writer transaction で増やし、競合する resume の winner を一つにする。
submit / resume controller は返された attempt を worker 起動・失敗処理にも渡す。
PID 登録の直前と failure commit は同じ writer transaction 内で expected attempt を照合し、
期限切れ queue の再開後に遅れた旧 controller が新 attempt を起動・失敗へ変更することを
`JOB_INTERRUPTED` で拒否する。旧 by-ID Rust API は互換として残すが、service の入口と worker の
failure completion は expected-attempt API を使う。
取消を明示的に再開する場合は cancel flag を reset し、その後の取消は新 worker の通常の
checkpoint / publication fence が扱う。

destination の同じ volume に `.kronello-job-{job}-{input_hash}-{attempt}` directory を作る。
`owner.json` は job ID、固定 input hash、snapshot hash、attempt を記録する。
通常の失敗では owned guard が削除し、hard kill 後は resume が同じ identity を照合する。
再実行 worker が heartbeat を動かしたまま旧 attempt を回収し、照合で完成を認めた場合の
回収も DB transaction の外で行う。違う marker、symlink、壊れた marker は削除しない。
attempt を path にも含め、古い worker の drop が新しい worker の出力を消さない。

公開済み artifact は従来どおり service が manifest / frame metadata / byte hash、または
movie stream / codec / PTS / duration / render・export snapshot identity で検証する。
公開前に state root の `job-results/{job}-{attempt}.json` へ全 artifact の長さ・SHA-256 と result、
owner identity を保存する。receipt は出力 directory に含めず、file を fsync し、Unix では
その親 directory も fsync する。artifact hash・receipt の streaming 書き込みと hash 計算は
DB connection / writer transaction の外で行う。receipt の byte SHA-256 だけを短い state DB
transaction へ commit してから、
次の短い transaction 内で lease / attempt / cancel を再確認し NOREPLACE rename を行う。

rename 後、succeeded の DB commit 前に停止した場合、resume は anchored receipt hash と
artifact 全体の hash、固定 snapshot に対する metadata を再検証する。
検証は DB transaction の外で行い、短い compare-and-set で status、PID、生存、attempt、input / snapshot /
publication hash と destination の metadata signature を再照合する。
一致する場合だけ新 worker を起動せず succeeded に補正する。異なる既存 output は削除・
上書きしない。destination が無い場合の部分書き込み receipt は job-owned directory 内で
回収し、通常の再実行を許す。receipt 名も attempt で分け、古い worker の部分書き込みが
新 attempt を妨げない。無検証の status 補正は行わない。

receipt-backed succeeded record の巨大な result report は DB record に重複保存しない。
公開 API の `JobRecord.result` は connection と process gate を閉じた後、hash を検証した
receipt から hydrate する。内部の `result: null` は公開時に解決し、active job には未確定 result を
見せない。receipt は input / log の retention prune 対象 directory の外に保持し、prune 後も
従来の歴史的 result 契約を維持する。旧 embedded result record はそのまま読める。

完成成果物は immutable artifact として扱う。DB と filesystem 全体を一つの transaction に
できないため、最終 hash 後に外部 process が成果物内容を書き換えることを封じる契約ではない。
metadata signature と再開時の再検証は行うが、外部改変を無条件に許可・修復しない。

## 資源制限と互換性

movie の hash は 64 KiB buffer で streaming 計算し、file 長も読み取り前後と照合する。
artifact tree は root の movie file または flat image sequence に限定し、symlink と入れ子
directory を拒否する。既存の最大 1,000,000 frame に合わせ file 数は 4,000,001 以下、
relative path は 4096 byte 以下、manifest の保守的 memory budget は 256 MiB 以下とする。
receipt は 512 MiB、owner marker は 16 KiB 以下。超過は型付き error とする。

job record の `attempt` と `publication_hash` は serde default により既存 record を読める。
旧 interrupted job は初回 resume で attempt 1 を使う。新規 receipt が無い既存成果物を
無条件に succeeded とみなさない。固定 envelope schema version 1 は維持する。

## 検証

[RECOVERY-001 検証記録](../testing/recovery-001.md)に実 worker の強制停止、公開境界の故障注入、
metadata 改変の拒否、asset / font / 意味版再検証、取消・lease・容量不足・GPU loss の回帰を記録する。
