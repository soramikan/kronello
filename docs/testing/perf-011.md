# PERF-011: 音声mix共有と長尺source準備

状態: `in_progress`。[全依頼計画](performance-export-plan.md)のPERF-011を実装・検証中であり、PERF-012〜014も残る。

## 同一音声の共有

`export_audio`は固定render hash（target/全plan/意味版/source lock/gain/effectsを含む）、audio envelope、audio mode、ordered clips、sample範囲、channel layout、clipping方針をSHA-256 keyへ逐次serializeする。codec/container/background/chaptersはPCM24の同一性へ含めない。document planとDSP/WSOLA sessionをgroupごとに一つ作り、mix/量子化を一度だけ行う。

mix quantumは最大4096 framesかつgroup内の最小codec block以下。各codecに渡すblockは小さいadapterで元のnative block長を維持し、partialを最後に一回だけ送る。整列済みのfull blockは借用sliceを直接送る。adapterはcodecが宣言した最大65,536frames×8channels以下の一blockで、全source長のPCMを保持しない。実codec内部の保持とpayload/RSSを区別する。

[実測](perf-011-measurements.json)・[独立packet再検証](perf-011-packet-identity.json)は実`render.submit`→`succeeded`、Metal、固定fixture、交互3pairsずつの初期比較を含む。beforeはPERF-010 `e188f8ae…`、共有mix afterは`f6296ffd…`。源資料は`target/perf-011/`に保存してhashを固定する。静穏hostを保証せず、一部で検証/buildが並行したため短いwall差を一般保証と扱わない。

| ケース | mix frames before→after | mix blocks before→after | adapter peak bytes |
|---|---:|---:|---:|
| MOV/FLAC・2000frames | 4000→2000 | 2→1 | 69,632 |
| PCM24/FLAC・1秒 | 96,000→48,000 | 23→12 | 69,632 |
| PCM24/FLAC/MP3/ALAC/AAC/Opus・1秒 | 288,000→48,000 | 174→50 | 119,808 |
| WSOLA＋HPF＋compressor・開始2000/終了8000 | 24,000→6000 | 17→7 | 85,504 |

初期24worker trialsで全decoded PCM/画素、PTS/DTS/duration、layout、codec priming/discard padding、snapshot hash/tags、chaptersと全compressed packet hashが一致した。WebMのcontainer bytes差をpacket一致と区別する。codecごとの最終partial長も元のblock編成を維持する。

単体3件はadapterの複数quantum/block組合せ、全sample順、single final partial、byte予算、失敗伝播、共有keyの差異を確認した。native integrationの`shared_audio_preserves_clipping_and_native_blocks_against_independent_exports`はFLAC/MP3の共有を独立exportの全file bytes・6000frames・clipping数と比較し、Reject時に両出力が未公開であることを確認した。media audio/audio5/audio10/deliveryは30 passed・2 ignored（その後追加した上記1件も成功）。

## spool再利用の評価

既定は従来のprivate spool。`with_audio_spool_cache`または実行policy `KRONELLO_AUDIO_SPOOL_CACHE_ROOT`を明示した試作だけが外部cacheを使う。request/作品/hashへpolicyを追加しない。cacheは8 entries/2GiBのPCM payload、headerは各64KiB、indexは4096 source framesごと32bytesで全PCM quotaに比例した小さい上限になる。

keyはexe bytes、FFmpeg capabilitiesと実library bytes、asset全文hash/stream metadata/index、decode cache版。cache参照の前後で元sourceの全文hashとfingerprintを検証し、全authored source rangeの検査を既存planに残す。range decodeを追加せず、coldでは従来どおり原点から全量decode/resampleする。prerollやDSP/WSOLAの状態遷移も変えない。

OS file lockでwriter/validatorを排他し、競合時はprivate spoolへ戻す。stage内の完成した不変PCMからhard link（別volumeはcopy）し、headerを最後に確定する。private stageを削除してもcacheのreaderは存続する。quota/件数でLRU退避し、live readerが除去を拒否する環境ではadmissionを見送る。writer leaseを取得してから自分のpublication temporaryだけを清掃する。

最初の全PCM再hash方式`d1cbb80c…`はWAV601秒→短区間の初期3pairsでcold1348ms、warm p501178msとなり、private spoolのwarm816msより遅かった。毎回の全PCM検証が約584〜587msを占めたため採用しない。

第二試作`284b2e17…`は小さいindex全文を検証し、読み込む4096frame windowのchecksumをsample返却前に検証する。未参照PCMを読み直さないが、元sourceは全文hashで検証する。参照windowの破損は型付きSourceRead失敗として公開を止め、そのcache headerを退避して次回原点decodeへ戻す。全量source decode・長さとlayout・authored range検査を省く方式ではない。

WAV初期3pairsはcold1343ms、warm p50806ms、before875msだった。ただし各warmは2試行でp95差も大きく、既定採用の根拠としない。圧縮FLAC sourceの9pairs（18 fresh workers）ではprivate spoolのwarm p50309.6ms/p95352.6msに対し、再利用はp50228.4ms/p95254.2ms、cold816.0ms（before308.5ms）だった。後続8試行では毎回source decoder0・spool write0、coldは全量decode/保存1回で、全decoded sample・PTS・packet hashが一致した。cold費用が大きいため既定で有効にせず、反復する長尺source向けの任意policyとして評価を継続する。最初のCLI指定`cpu`は受理されなかったため、実行可能な`cpu-reference`の`…-v2`を証拠にした。

cache単体2件ではstage cleanup後の再利用、参照PCM破損の拒否/header退避/次回再decode、元source書換え拒否、quota超過とwriter競合時のprivate spool継続を検証した。source fieldとnative block adapterを含むmedia unit25件、media all-targets Clippyは成功。

## 残る受け入れ

- 圧縮source比較とcold/warm・CPU/I/O競合を評価してcache採否/対応範囲を確定する。
- cache index/header欠落・破損、eviction、cancel、途中publication回収、runtime変更の追加試験を完了する。
- layout/audio mode/gainの異なるgroupで非共有と各sampleを実worker検証する。
- ADR/現在の設計、fmt・全workspace Clippy/testsを完了する。すべての受け入れ前にbacklogをdoneにしない。
