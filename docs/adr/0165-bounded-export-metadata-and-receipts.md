# ADR-0165: 長尺frame metadataとpublication receiptの有界化

- 状態: 採用（movie/GIFの接続を実装、PERF-013受け入れ進行中）
- 日付: 2026-10-12
- 対象: PERF-013
- 後続: ADR-0079、ADR-0133

## 問題

movieの画素payloadは二組のbundleへ収まるが、`AvExportReport.frames`、frame時刻/PTSのVec、workerのJSON化/clone、receiptの`artifacts` Vecはframe数・file数に比例する。metadataだけを分割してもreceiptの一覧を全量hydrateすると受け入れを満たさない。

## 決定する境界

- 短いreportは従来のinline metadataを維持する。inlineは件数とserialized byteの両方で上限を持ち、それを超えたreportは版付きmanifest descriptor（hash、byte数、frame range/count、chunk count、snapshot identity）と小さいsummaryを返す。全chunk参照をroot reportへVecで置かない。
- 共通headerへsnapshot/hash/意味版/font locks/画像形式などを一回保存し、chunkにはframe時刻/index/sequence number/転送・cache・temporal情報などの差分を保存する。再構成した`FrameMetadata`を従来の型で検証し、共通部分の変化を黙って省略しない。
- writerは固定件数/byte上限の一chunkだけを保持する。chunk indexもfileへ逐次書き、rootへindexのhash/countを記録する。header/chunk/indexには個別のread budgetを設ける。初期候補はinline64件/1MiB、chunk64件/4MiB、header8MiBであり、既存の最大temporal/lockケースを実装時に照合して確定する。
- frame planは有理数の開始frame/countと逐次iteratorで表し、全時刻・PTSのVecを作らない。範囲は従来の`[start, end)`と正確なframe境界を維持する。
- 新receiptはartifact entryを有界にserializeする外部indexのhash/countを持つ。root receiptに全artifactを置かず、publication/recoveryはindexを順に検証する。出力legとmetadata領域を固定上限のpublication root群として扱い、entryはroot番号と相対pathを持つ。metadataもartifact setのidentity/hash/final fenceに結び付け、movieだけのrename後に別の無検証manifestを保存する順序にしない。
- file名/相対pathは実装が生成し、regular file、所有identity、深さ、byte数、count、連続するframe rangeを検査する。欠落/破損/順序違い/途中publicationを型付き失敗にする。入力から任意file pathを指定させるAPIを追加しない。
- `job.get`/`job.list`はsummaryを返し、詳細はjob identityとoffset/limitに結び付いたread-only Queryでページ単位に読む。pageも件数/byteの両方を制限する。旧inline report・旧receiptを読む分岐を維持し、復旧を新形式だけの試験で済ませない。
- 同期exportと固定workerで同じmetadata writer/readerを使う。失敗/取消/lease喪失では未確定manifestを回収し、成功後のreaderがstage cleanupで壊れない格納・確定順を実装する。

## mux確定工程

private stageの既取得probeを渡してRust側の重複open/検査を除く。native muxはpacket処理中に有界な取消確認を行う。最終containerのprobe、PTS/DTS/duration、priming/padding、chapters、snapshot metadata、hash、lease/final cancel fence、no-clobber publicationは維持する。GIFのglobal palette二段階は独立した経路として扱う。

## 受け入れ前に必要な証拠

短い旧reportとの全metadata復元比較、境界を跨ぐページと破損拒否、長尺の完走とwriter/reader/receipt/queryの保持量、旧receipt復旧、取消/kill/lease喪失/途中確定の回収、mux全packet/時刻/chaptersの一致を確認する。movie/GIFの共通header/chunk、root receipt/index、page Queryは実装済みだが、image sequenceとmux/追加復旧検証が残る。全体の受け入れ済みとは扱わず、範囲ごとの証拠を[PERF-013](../testing/perf-013.md)で追跡する。
