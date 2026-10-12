# ADR-0166: 画像連番のmetadataと画素参照を有界manifestへ保存する

- 状態: 採用（PERF-013受け入れ済み）
- 日付: 2026-10-13
- 対象: PERF-013
- 部分置換: ADR-0165のimage sequence除外とADR-0050の画像連番全frame結果格納。短いversion1、既存receipt読み込み、画素形式・時刻・hash・固定snapshot・lease/cancel/final fence・NOREPLACEは維持する。

## 問題

movie/GIFのmetadataを分割しても、画像連番は全frameのmetadata・画素file参照をreportとreceiptへ保持する。個別frame JSONとsequence JSONに同じmetadataを重複保存するため、file数と結果保存の費用も増える。

## 決定

- 画像連番にもADR-0165のinline64件/1MiBと共通header・64件/4MiB chunk・逐次indexを使う。閾値以内のversion1は従来のnumeric/display/frame JSONを維持する。超過時はversion2で全frame Vecを空にし、descriptorだけを返す。時刻列は定数サイズの有理数frame planで生成する。
- 差分chunkにnumeric/displayの生成名・byte数・SHA-256を保存する。予約key `__sequence_files` はFrameMetadata復元時に分離し、`job.frames`の`files`を`frames`と同じ順序で返す。movie/GIFではこのoptional keyを省略し、既存manifestも読める。
- file名は絶対frame indexから生成したordinalに一致させ、相対pathの任意指定を受け付けない。snapshot/range/rate/countとnumeric byte数を検証し、publication/recoveryでは各画素fileを64KiB bufferで逐次hash照合する。metadataだけを復元するpage Queryと全payload検証を区別する。
- 連番の`frames` directoryはprimary output directoryと同じrootとして移動する。最終pathへrelocateした小さい`sequence.json`を先に保存してからreceiptのartifact indexをhashし、directoryのNOREPLACE renameとfinal fenceで公開する。取消時のRAII回収とkill後の所有identity検証によるstage回収を維持する。
- root receiptとstatusに全frame参照を戻さない。成功後のmanifestとartifact indexはjob input/logのpruneから独立して残し、ページ取得を維持する。旧version1の全量読込は互換経路として残るが、新しい長尺writerの保持量と混同しない。

## 検証

全semantic metadataと両画素payloadの復元、境界63/64のpage、長尺worker完走・RSS、chunk保存後の取消/kill/resume、rename後chunk破損の拒否、旧receipt復旧、schema/生成Swift・必須workspace検証を[PERF-013](../testing/perf-013.md)に記録する。採用決定だけで全受け入れ済みと扱わない。
