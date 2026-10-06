# PERF-001 media seek 検証

[ADR-0091](../adr/0091-exact-forward-decoder-and-bounded-render-scope.md)の正確な順方向保持と service sequence/movie 範囲の有限寿命 decoder を検証する。

## 正確性と製品経路

```sh
cargo test -p kronello-media --test media --locked -- --nocapture
cargo test -p kronello-media --test composition sequential_product_backend --locked -- --nocapture
cargo test -p kronello-media --test media negative_origin --locked -- --ignored --nocapture
```

通常テストは既存の 24/25/30/30000÷1001/60000÷1001 CFR、VFR、B-frame の start/middle/end直前、逆順・反復・範囲外を確認する。新テストは独立した fresh decoder との native planes/PTS/end/color の完全一致、6 フレーム順方向で seek=1/decode=6、反復20回で interval_hits=20/decode増加なし、2 native frames=768 bytes の保持、範囲外・メタデータ照会後の回復を確認する。

製品 backend テストは Composition Media を共通レンダーへ渡し、従来 backend と `SequentialVideoRenderBackend` の全 linear/display pixels を完全比較する。順方向後の逆方向シークと反復命中を確認し、命中区間の source を変更すると `ASSET_HASH_MISMATCH` を返す。

負の origin は固定 developer FFmpeg コマンドで生成した MPEG2 B-frame TS を使う明示テストである。LGPL runtime の既存 native `mpeg2video` decoder を利用し、配布 codec 構成は変更しない。timestamp seek 成功後 EOF となる negative-origin TS では同一 stream を再 open する explicit exact restart も検証する。native PTS は -3/24,-2/24,-1/24,0,1/24,2/24 であり、native の正確な `[pts,end)` と独立 decoder 比較を確認する。製品 API は任意 FFmpeg 引数を受け取らない。

## release 測定

変更前に release example をビルドし `target/perf-001-evidence/seek-benchmark-before` に保存した。変更後は同じ example の production `decode_at` をビルドする。

```sh
cargo build -p kronello-media --example seek_benchmark --release --locked
target/perf-001-evidence/seek-benchmark-before before
target/release/examples/seek_benchmark after
```

CFR/VFR/Bframe の forward/backward/repeated を各5回 warmup、60回測定する。native runtime はロード済み、各 repetition は fresh decoder とする。時刻を保持する product backend は同じ production decoder を呼ぶ。oracle のロード/復号・全画素比較は測定から除く。raw JSON に全 samples_ns、p50/p95、seek/decode/hit/保持byte、fixture hash、組み込みソース hash、runtime/codec/build/host を保存する。

測定結果は `perf-001-media-seek-measurements.json` に別途記録する。前後 binary の構築/検証は並行作業がある時間帯でもよいが、数値測定は root/GPU/他担当の build/test/GUI が停止した区間に限定する。native planes 保持上限は codec 内部メモリーや変換バッファを含む RSS 上限ではない。

## 2026-10-06 実測結果

同一 Apple Silicon macOS / release / 配布 FFmpeg runtime で実行した。root・GPU・platform の重い処理停止後、保存した before binary→after binary の順に測定した。温度計測はしていないため温度一定とは主張しない。runtime/codec・ホスト・revision/dirty files・compile 時ソース hash と全 raw samples は [測定 JSON](perf-001-media-seek-measurements.json) が正本である。before binary の fixture generation 文字列だけ変更前の誤記 `generate-media` が残り、実際の生成コマンドは `generate` と JSON に訂正を記録した。

| ケース | p50 before→after (ns) | p95 before→after (ns) |
|---|---:|---:|
| CFR/forward | 4583→1375 | 6666→3750 |
| CFR/backward | 4333→5250 | 6792→8500 |
| CFR/repeated | 4917→83 | 5542→1625 |
| VFR/forward | 3250→958 | 5291→2708 |
| VFR/backward | 3500→4208 | 5625→6583 |
| VFR/repeated | 4209→83 | 4792→167 |
| Bframe/forward | 7750→1333 | 9875→8334 |
| Bframe/backward | 7792→7666 | 13375→12959 |
| Bframe/repeated | 8959→83 | 12333→166 |

全9ケースで全 native planes/PTS/end/color が完全一致した。順方向と同区間反復を改善し、逆方向は同じ origin 復号を維持するため小フレームでは保持/複製のコストで遅くなった。逆方向が高速化したとは扱わない。16×16 の固定 correctness fixtures における decode_at の数値であり、8K/長尺の throughput へ外挿しない。

`/usr/bin/time -l` の process max RSS は before 16,056,320 bytes、after 16,138,240 bytes、peak memory footprint は両方4,817,376 bytes、swap は両方0だった。プロセス全体 wall time 0.14→0.38秒は oracle/起動/スケジューリングも含み、seek 改善の指標には使わない。CFR/VFR/Bframe 順方向の各60反復で seek360→60/decode1560→360、反復区間は seek1200→60/decode7200→360（fixture5番目の隣接終端用 frame まで復号）となった。保持 native planes の peak は decoderあたり768 bytesで固定された。


`cpu_copy_bytes` は native→Rust planes に加え、新しく保持する current の clone と interval-hit の戻り値 clone も含む。forward は各60反復の総量599,040→276,480 bytes、repeated は2,764,800→599,040 bytes、backward は599,040→737,280 bytes。コピー低減を主張する場合も追加の clone を除外しない。`cache_clone_bytes` と `returned_clone_bytes` を分けて raw JSON に記録した。戻り値の一時コピーは `peak_cached_frame_bytes`（保持のみ）には含めない。

## 1080 source を用いる movie 製品経路

```sh
cargo build -p kronello-media --example product_seek_benchmark --release --locked
/usr/bin/time -l target/release/examples/product_seek_benchmark before
/usr/bin/time -l target/release/examples/product_seek_benchmark after
```

同一の production `export_av`（fixed snapshot、選択済み CPU-reference backend、native ProRes/PCM24 encoder/mux/probe）を通し、従来の frame ごとの `VideoRenderBackend` と service の `with_video_backend` が実際に保持する `SequentialVideoRenderBackend` を比較した。before/after は同じ最終 compiled binary の backend 選択であり、別の簡易 exporter ではない。素材は配布 LGPL runtime で生成した1920×1080 ProRes HQ / native yuv422p10le /6frames、PTS=i/24、固定不透明 RGB の recipe である。出力128×72へ縮小し、decode/変換を中心とした比較にした。1080の最終描画 throughput、4K、長尺、GPU の数値へ外挿しない。

両 phase は別プロセスで3回 warmup、20回 export を測定した。各 export の最終 movie 全フレームの native planes/PTS/end/color を測定外で完全比較し、別プロセス間でも source SHA256 と final movie native identity が一致した。movie export p50 は332,001,708→264,447,084ns（20.3%短縮）、p95は346,862,667→269,203,125ns（22.4%短縮）だった。native codec の内部 peak は FFmpeg から取得できないため unknown と明記し、完全処理の観測 RSS と混同しない。

- after 各 measured export の実測 counter: decoder1個、miss1/hit5、seek1/decode6、eviction0。current/lookahead 保持 peak16,588,800bytes、終端時8,294,400bytes。current clone は49,766,400bytes、interval-hit clone は0。before の wrapper は decoder counter を公開しないため product の before decoder counter は未取得（0 と解釈しない）。独立 microcase の前後 counter は前節にある。
- 保持資源の実装上限: decoder2個、呼び出し間保持128MiB。通常テストで3つの異なる資産 lock の投入による count2/eviction1 と retained bytes を検証した。上限超過は同じ source の保持を捨てるだけで、近似 seek/別 codec/CPU fallback を選ばない。
- 源素材の native frame は8,294,400bytes、current/lookahead はその2倍。SDR変換のRGBA8一時配列8,294,400bytes、VideoImageのlinearRGBA配列33,177,600bytesは保持planesとは別に発生する。戻り値のowned planesも別である。計測された保持量にこれらを加えて「decode全peak」と呼ばない。
- 出力 encoder の frontend RGBA8 buffer は36,864bytesで1frameのbackpressure。render linear/display のサイズ上限見積もり294,912bytes。native encoder/decoder 内部割り当ては未公開である。観測 RSS には source生成・oracle・codec・render/cache・muxの全区間が含まれる。
- `/usr/bin/time -l` の max RSS は before97,206,272→after97,402,880bytes（0.20%増）、peak memory footprint76,759,568→76,726,800bytes、swap両方0。プロセス時間8.26→6.37秒は起動・oracleも含む補助値である。

共通 service の sequence/movie/fixed job は同じ scope wrapper を使う。pure model/snapshot/RenderCache に native handle を入れていない。wire/profile意味版/ファイルschemaの変更はない。
