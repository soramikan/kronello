# PERF-002: 描画頻度から独立した通知確認と最適化ビルド

## 受け入れ範囲

2026-10-11、[ADR-0143](../adr/0143-deadline-driven-native-notifications.md) の通知期限と Rust/Swift release configuration を実装した。参照チャット「パフォーマンス改善検討」の最初の固定費削減であり、全4段階の完了ではない。snapshot/font/LUT・decoder・proxy/UI は PERF-003、export/audio は PERF-004、長尺/resident は PERF-005 に分ける。

## 実装

- 定期確認は前回の確認終了から250ms後。連続した描画/queryでも期限到達時に確認する。購読開始と成功した文書/ジョブ変更後には即時確認する。
- audio capture の要求が続く場合も期限確認を行う。completion と通知の共有 JSON、64件のbackpressure、最新通知への集約、safe session の排他を維持する。
- `KRONELLO_FFI_TIMING=1` は要求ごとの queue wait / execution と確認ごとの notification 時間を stderr の `ffi_timing` JSON に出す。通常時は出力しない。作品・資産の文字列は出さない。
- `scripts/build_macos_app.py --release` は Swift build と bin path の両方に `-c release` を渡す。`--skip-build --release` も release 成果物を選ぶ。

## 実測

`scripts/measure_ffi_polling.py` は生成した100/1000/5000クリップの作品で native worker に軽い型付き失敗要求を連続送信する。GPU/GUI表示・映像decodeを含まない。購読直後の1要求を除き101要求を集計する。素材を含まない生成作品で、FFIライブラリはreleaseの同じ作業ツリーを使用した。元からあった別作業の変更を含むため、HEADだけで再現できる計測ではない。

初回の調査値（ms、購読あり）：

| clips | 変更前 p50 / p95 | 期限分離後 p50 / p95 |
|---:|---:|---:|
| 100 | 3.069 / 4.211 | 1.137 / 6.452 |
| 1000 | 27.689 / 29.965 | 0.205 / 2.155 |
| 5000 | 258.056 / 582.123 | 0.170 / 0.178 |

購読なしの中央値はおおむね0.13〜0.17ms。これらは他のビルド/検証が動く環境での探索測定で、quiet hostの安定した改善率ではない。2回目の変更前測定にもホスト負荷があり、ファイル名 `ffi-before-quiet.json` はquiet条件の成立を示さない。生の要求時間と全試行は [測定JSON](perf-002-measurements.json) に保存した。

要求間に16.667msの間隔を置き、定期確認を含めた期限分離後の測定（ms）：

| clips | p50 | p95 | 意味 |
|---:|---:|---:|---|
| 100 | 0.218 | 0.592 | 小作品の定期確認 |
| 1000 | 0.216 | 13.390 | 定期確認の仕事量は残る |
| 5000 | 0.187 | 175.683 | 大作品ではrevision単位の準備再利用が必要 |

短いburstのp95を再生全体の保証にしない。この残存する待ちをPERF-003の優先事項とする。

```sh
cargo build --release -p kronello-ffi -p kronello-cli --locked
python3 scripts/measure_ffi_polling.py --output target/perf-improvements/ffi.json
KRONELLO_FFI_TIMING=1 python3 scripts/measure_ffi_polling.py \
  --interval-ms 16.667 --output target/perf-improvements/ffi-paced.json
```

## 検証

- FFI `boundary` の11テスト成功。外部revisionとjob通知のテストを、要求が続く状態でも通知される検証へ拡張した。sandbox内では既定job状態へのアクセスが制限され失敗したため、権限のある実行で確認した。
- Swift release build 成功、bin path は `.build/out/Products/Release`。既定CommandLineToolsのSDK/compiler不整合を避け、`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`、書き込み可能な `/tmp` module/cache path、`--disable-sandbox -debug-info-format none` でコンパイルした。debug-infoの生成権限により初回はdSYMで失敗した。GUI操作時間とdebug/releaseの速度比は未測定。
- sandbox内のworkspace testはMetal adapterが見えず失敗した。GPUにアクセスできる環境の再実行では通常テスト1,289件が成功（44件ignored、受け入れ成功に数えない）したが、実行中の別作業によるモデル型追加と古い依存バイナリの不一致でstoreのdoctestが失敗した。現在のsourceを再buildした `cargo test --workspace --locked --doc` は成功した。同一sourceでのworkspace test一括成功は後続の性能変更の最終gateに残す。
