# PERF-010: 色変換直接書込みと複数出力共有

状態: `done`。PERF-006〜014の全計画は[依頼計画](performance-export-plan.md)で追跡する。初期比較と追加検証を区別し、全体検証の完了後に受け入れた。

## 実装

`export_color::packed_into` は既存bundleのstride/offsetへ直接書く。SDRのf32 composite・BT.709 transfer・byte丸め、HDRのf64 composite・PQ/HLG・u16丸め、HDR→SDRの旧f32中間保存境界を維持する。一時packed Vec、行copy、tone-map中間配列を作らない。書き込み前にpixel count・row stride・offset・destination長のchecked算術を検査する。

背景のf32 bit列・transfer・tone map・packed formatが一致するlegは一つの変換bufferを共有する。owner以外のbundle slotは空であり、producer/queue/consumer/finish全体で最大二組のowner bufferだけが循環する。GIF palette/spool/第二段階もowner bufferを使い、追加frame allocationを作らない。

動画はMovieだけを対象に、共通raster・frame grid/PTS/span・固定bitrate/codec threads・選択済みruntimeに加え、変換owner・video codec・HDR transfer・DNx profileが一致する場合に一つのnative encoderとprivate video stageを共有する。最終container、音声、chapters、export hash、probe、no-clobber publication、cancel/lease fenceは各legに残す。共有legのvideo reportは実行pathを示し追加transfer bytesは0とする。native handleをthread間共有しない。

native RGBA→YUV変換は既存の独立stage計測を使う。`KRONELLO_ENCODER_SWS_THREADS` は閉じたcompile-time比較値1/2/4で、requestへ追加しない。source decode converterは変更しない。codec内部はProRes4/DNx2、SVT `lp=1`を維持する。比較で高速化を確認できなかったため既定の変換threadsは1を維持し、2/4は製品の選択肢として追加しない。

## 初期実測

[初期測定](perf-010-measurements.json)は固定4K実動画・1frame・3本のProRes出力、Metal、A/B交互3pairs（6回の実worker）である。beforeはPERF-009の`49df8a6d…`、afterのthreads=1は`f9f9a432…`。各試行は新しいworker、CLI `render.submit`からDB `succeeded`までを計測し、独立decodeは計測外。静穏hostを保証せず、この短い試行を一般的な速度保証や同worker warm測定と扱わない。

| 項目 | before | after |
|---|---:|---:|
| 色変換回数/frame | 120 | 40 |
| native video encode回数/frame | 3 | 1 |
| 二組のbundle payload bytes | 199,065,600 | 66,355,200 |
| 初回worker wall ms | 1,984.2 | 2,141.5 |
| 後続worker p50 ms（各2試行） | 1,921.5 | 1,747.7 |

全pairでrender/export snapshot hash、独立decoded画素/音声/PTS/DTS/duration/layout、chapters、packet side data、encoded bytesが一致した。3出力の章数は1/0/1で、共有後も各legのtransfer/omit指定を維持した。payloadはRSS・native codec/driver保持と区別する。

比較ハーネスは`-show_chapters`とpacket side data（priming/discard paddingを含む）を取得するよう修正した。初回の`duplicate-chapters-sws1`はMOVの章data streamをPCMと誤認して独立decodeが失敗した。workerは成功しており、data streamをdecodeせず章を別比較する修正後の`…-v2`を証拠とする。過去のPERF-006〜009の証拠を拡張検証済みと書き換えない。

## 追加検証（初期17比較）

固定fixtureを交互に3pairsずつ実行し、11ケースの変更前後比較66worker trialsと、ProRes/DNx/AV1の変換threads1→2/4比較36trials、合計102trialsを完了した。[工程要約](perf-010-stage-summary.json)と[全測定](perf-010-measurements.json)を参照する。native helperを含むmedia unit/delivery/HDR/profilesは33 passed・2 ignored。fmt、backlog check、all-targets workspace Clippyは成功。workspace testsは最終sourceで成功した（下記）。

- 同じ4K入力のDNx MOV/MXFは変換80→40、encode2→1、bundle132,710,400→66,355,200 bytes。独立decoded bits・PTS/DTS/duration・layoutは一致する。
- AV1 ALAC/AAC/Opusの3container・12framesは変換432→144、encode36→12、bundle49,766,400→16,588,800 bytes。各audio codecのdecoded sample、PTS、packet priming/discard padding、snapshot metadataを維持する。
- PQ/HLGごとのHDR duplicate＋SDR tone-map duplicate・4framesは変換32→16、encode16→8、bundle11,059,200→5,529,600 bytes。全decoded sample/画素・transfer tags・compressed packet hashが一致する。異なるPQ/HLGを同一fixed renderに混在させるfixtureは既存の型付き検証で拒否されたため、受け入れ対象を各fixed transferに分けた。このguardを緩めていない。
- 半透明の別背景は変換80・encode2を維持し、独立画素を変更前後で一致させた。GIFは変換だけを共有しnative movie encode数とpalette/spool/第二段階を維持する。document audio/silenceの2出力でも音声は独立する。
- MXFでは`material_package_umid`が異なり、WebMでもcompressed packet外のcontainer bytesが変わってfile全体hashは一致しない。[保持出力のpacket再検証](perf-010-packet-identity.json)で全17比較の全compressed packet payload・extradata・有理数PTS/DTS/duration・priming/padding・color・chapters・snapshot tagsのhash一致を確認した。元のfile hashを前後に照合し、timing receiptを書き換えず独立証拠を追加した。encoded file一致とpacket一致を区別する。
- 変換threads1/2/4の要求CPU予算はProRes codec4と合わせ5/6/8、DNx codec2と合わせ3/4/6。SVT codec1/`lp=1`は固定。要求option数を実際の全OS thread数/RSS上限と扱わない。変換工程のp50は4K ProResで約16.9〜17.7ms、DNxで約16.7〜17.4ms、12frame AV1で約24.8〜25.4msで、2/4の改善を確認できなかった。既定1を維持し、2/4を一般設定へ追加しない。各試行のRSSとwallは工程要約に別記する。

初期失敗はfixture/harnessの修正である。MOV chapter data streamの誤decodeに加え、AV1の`.mov` destinationは`.mp4`へ修正して`…-v2`で再実行した。混在HDR transfer fixtureの拒否と同様、productionの型付き失敗を成功へ書き換えていない。

## 別encoder・VideoToolboxと最終source

さらにProRes/DNx別encoder（4K・4frames）とVideoToolbox H.264 ALAC/AAC（640×360・4frames）を3pairsずつ比較し、合計19比較・114成功worker trialsに拡張した。ProRes/DNxは色変換320→160、encodeは8のまま、bundle132,710,400→66,355,200 bytes。afterのbuffer acquireは全4frames合計1µs、submit waitは19µs、consumer input waitは5.30s、native conversion/codec sendは約0.34sで、consumerがrender入力を待つ側だった。この測定ではleg別worker追加の条件であるencoder待ちの支配を確認できず、二組・一consumerを維持する。hostでworkspace検証とbuildが並行していたためwallの改善率を因果効果として扱わない。

H.264はframe/PCM bits、PTS/DTS/duration、color、AAC priming/padding、container snapshot tagsが一致した一方、raw packet hashは先頭のVideoToolbox private unregistered-user-data SEIだけ異なる。VCL sliceは一致した。原packet hashを保持して`all_packet_payload_equal=false`と記録し、画素/時刻の比較から圧縮bytesを分離した。再検証は明示的`--normalize-vt-private-sei`を使い、UUID `47564adc5c4c433f94efc5113cd143a8`・payload type5/45bytes・単一SEI/trailing bitsを照合したNALだけ除外する。他SEI、SPS/PPS、slice、音声packet、extradata、時刻、padding、color、chaptersはそのまま比較する。全19比較の宣言した比較は成功し、H.264だけ`all_packet_payload_time_color_chapters_equal=false`と正規化flagを残す。raw packet/file全体一致を主張しない。最初のH.264比較はraw packet hashも画素と同じassertionで扱ったため失敗したが、decoded hashは初回から一致していた。ハーネスを修正して`…-v2`で再実行した。

通常設定の最終release binaryは`e188f8ae662430611006d4b2c6de4a45e898aaf3c2fdfa74ea0d598ba0c0b11c`。初期binaryとの差は`cfg(test)` module配置修正で、runtimeの直接変換/共有/native policyは同じ。H.264の最後の6trialsはこのbinaryで行った。

## 最終受け入れ

`a60e071`のsourceで`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`python3 scripts/backlog.py check`が成功した。workspaceは200 suites / 1,357 passed / 0 failed / 47 ignored。ignoredは成功数に含めない。実native/GPUを必要とするテストはMetal利用可能なhostで実行した。[検証receipt](perf-010-validation.json)はログhashとsource commitを固定する。

全19比較114worker trialsの源資料、packet/semanticの異なる保証、threads1/2/4とencoder待ちの判断を上記に記録してPERF-010をdoneとした。全依頼は完了しておらず、PERF-011〜014を引き続き追跡する。
