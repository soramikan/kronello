# ADR-0091: 正確な順方向デコードとレンダー範囲の有限寿命

- 状態: 部分置換（[ADR-0140](0140-media-session-resident-preview.md) が「毎アクセス hash 検証」を実行時 fingerprint メモ化に、保持 byte budget を 512 MiB に更新）
- 日付: 2026-10-06
- 対象: PERF-001 の media decode
- 部分置換: [ADR-0048](0048-media-native-build-and-asset-verification.md) の毎要求 seek/flush と decoder 寿命の実装方針のみ

## 置換範囲

ADR-0048 の正確な native PTS/提示区間、資産 hash・stream lock、native planes、SDR/HDR色契約、LGPL 動的ロードと ABI 境界を維持する。変更は同じ exact decoder の順方向再利用と明示した origin restart、service 処理範囲の寿命に限定する。

## 決定

`VideoDecoder::decode_at` は直前の提示区間 `[pts,end)` と次の提示フレームを保持する。同区間の要求は同じ native planes を返し、順方向は保持した次フレームから復号を続ける。区間の終端は隣接する正確な PTS、最終フレームだけ正の native duration とする。CFR の推定周期、VFR の補間、GOP 長の推測を導入しない。

逆方向は既存の indexed stream origin へのシークを使い、そこから正確に復号する。負の origin を 0 へ丸めない。負の stream origin は timestamp seek の成功値が EOF を指す TS demux の問題を避けるため、同一 canonical file / stream を再 open して正確な先頭 packet から復号する。この分岐は明示した exact restart 方針であり、近似時刻・別 asset/codec への fallback ではない。統計の `seeks` はこの exact restart 回数も含む。範囲外・非増加 PTS・duration 不明などのエラー後は提示キャッシュを捨て、次回の要求を origin から再開する。メタデータ照会後も同様に再開する。戻り値は独立所有であり呼び出し履歴による意味の変更を許さない。

service の共通 `with_video_backend` は `SequentialVideoRenderBackend` を sequence/movie/fixed worker の処理範囲に置く。native runtime/decoder はこの範囲でのみ保持し、snapshot、純粋モデル、`RenderCache` へ格納しない。CPU/GPU の選択済み描画バックエンドと SDR/HDR の既存変換を共用する。strict resident 経路はその既存 native 契約を維持する。

デコーダーは最大 2 個、呼び出しをまたぐ native presentation planes は合計 128 MiB を上限とする。LRU で古いデコーダーを破棄し、1 個でも byte budget を超える場合はその保持も破棄する。これは高速化資源の破棄であり、別の source/backend への fallback ではない。各 decoder は current/lookahead の最大 2 native frames を持つ。native codec 内部のメモリーと戻り値・変換中の一時配列は別であり、保持 byte budget をプロセス全メモリー上限と呼ばない。

識別は canonical local path、stream index、資産の全 lock/content hash とする。区間命中時も毎回ファイルの hash を検証し、native format/dimensions/color の lock を同じ変換関数で検査する。native runtime が利用できない場合は実際の動画要求だけ型付きエラーにし、画像/空 composition の描画に新しい runtime 必須条件を加えない。

## 検証

[media seek 検証](../testing/perf-001-media-seek.md)に、独立 decoder との全 native planes/PTS/end/color 比較、製品経路比較、資源境界、release 前後測定を記録する。基準実行ファイルはアルゴリズム変更前に保存し、同一ホストの静かな測定区間で変更後と比較する。
