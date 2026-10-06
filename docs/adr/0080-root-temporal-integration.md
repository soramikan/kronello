# ADR-0080: root scope の有理数露光積分

状態: 採用
日付: 2026-10-06
対象: RENDER-002 / CACHE-002

## 決定

`RenderProfile.temporal` を省略すると従来の単時刻描画を維持する。
指定時は `TemporalSettings` の版 1 を snapshot の `SemanticVersions.temporal` に明示固定し、
GUI / CLI / MCP の共通 `render.frame`、`render.sequence`、movie export、固定 worker が同じ executor を使う。

露光幅は `shutter_angle / 360 / frame_rate`、開始は
`frame_time + shutter_phase / frame_rate` とする。角度と位相は有理数であり、
位相の単位は frame、標本点は各等幅区間の中点である。
標本数は 1..=4096、角度は 0..=360 とする。ゼロ角度は位相に関わらず
要求された nominal frame time の一標本、重み 1 とする。
同時刻の要求は有理数重みを加算して一回だけ実行する。

各 root 時刻で Composition 全体を合成する。ネストは通常の `TimeMap` で
同じ root 標本時刻を写像し、独自の shutter 標本を増やさない。
線形作業色空間の premultiplied RGBA を float64 accumulator へ逐次加算し、
最後に float32 へ変換して一回だけ sRGB straight 表示変換を適用する。
レイヤー別の平均は基準実装を置換しない。動画デコードは元の標本精度を維持し、
オプティカルフローを暗黙適用しない。

Sequence の `avoid_crossing` は nominal time を含む root 視覚編集区間へ露光区間を
切り詰め、再正規化する。時刻が境界と一致すると incoming 側の `[start,end)` を選ぶ。
Audio トラックは視覚露光境界にしない。接続された crossfade の開始・終了は連続な
重なりとして扱い、その clip の境界を hard cut として切り詰めない。
`allow_crossing` は切り詰めない。Composition 内のノード active range は
各標本の通常の可視性評価で扱う。

標本画像を同時保持しない。movie export は既存の tile sink を使い、tile ごとの
accumulator と現在の標本に限定する。累積 ROI halo の DAG 面予算と temporal
accumulation 予算を各 512 MiB で拒否する。単一 DAG の native preview と
`render.explain` が temporal plan を説明できない場合は型付き `UNSUPPORTED_FEATURE`
で拒否し、単時刻を temporal と称さない。

旧 snapshot の missing temporal pin は `None` として保持し、単時刻の場合だけ受け入れる。
temporal profile があるのに pin が欠ける場合は実行を拒否し、将来の意味版で補わない。
`require_gpu_resident` は現在の CPU accumulation と両立しないため明示拒否する。

## キャッシュ

backend が安定した numeric / device namespace を明示した場合だけ temporal 出力を保持する。
key は temporal 意味版、revision を除く snapshot 全内容、設定、全有理数標本と重み、
要求 ROI、各標本・tile の実行 ROI halo、backend namespace を含む。
したがって nested definition / TimeMap / effect / lock / working space の変更は失効する。
ヒット判定前に各標本の scene / DAG と font lock を検証する。
外部動画は実ファイル検証を backend に委譲するため temporal 出力 cache を利用せず、
各実行で backend を通す。LRU の entry / byte 上限を独立した temporal capacity で守る。

## 理由と影響

全体合成を基準に保つことで重なり・matte・effect の意味を維持する。
有理数の位相・標本を固定 snapshot に含めることで再開と実行順から独立させる。
出力 frame rate と shutter frame rate が異なる sequence / movie は実行前に拒否する。
CPU reference と GPU は同じ根の計画を使うが、device namespace をまたぐ出力再利用はしない。
