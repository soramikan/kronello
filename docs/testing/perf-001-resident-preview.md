# PERF-001 resident preview 検証

[ADR-0140](../adr/0140-media-session-resident-preview.md)の永続 `MediaSession`・検証メモ化・GPU resident アップロード経路が、8K 動画プレビューを 24 fps 予算（41.7 ms）内に収めることを検証する。

## 再現手順

配布 FFmpeg runtime の `hevc_videotoolbox` で 10 秒の 8K 素材を生成する（生成内容の一致性は測定対象ではない）:

```sh
target/native/ffmpeg-lgpl/bin/ffmpeg -y \
  -f lavfi -i "testsrc2=size=7680x4320:rate=24" \
  -f lavfi -i "sine=frequency=440:sample_rate=48000" \
  -t 10 -c:v hevc_videotoolbox -c:a pcm_s16le -shortest hevc8k.mov
```

release example をビルドし、未解決 DAG → budget-fit → resident 解決 → GPU テクスチャ化までの GUI が 1 フレームに支払う全経路を計測する:

```sh
cargo build -p kronello-service --example perf_video_preview --release --locked
target/release/examples/perf_video_preview <abs>/hevc8k.mov <abs>/none.kronello none 24 1512x851
target/release/examples/perf_video_preview <abs>/hevc8k.mov <abs>/blur.kronello blur 24 1512x851
target/release/examples/perf_video_preview <abs>/hevc8k.mov <abs>/blur1080.kronello blur 24 1920x1080
```

`blur` は `kronello.gaussian_blur`（sigma=4）をクリップに付与して、素の再生だけでなくエフェクト込みの経路を測る。raw JSON は全サンプルの stage 別 ms と decoder pool 統計を保持する。

## 2026-10-09 実測結果

環境: macOS 27.0.1 / Apple M4 / 32 GB / FFmpeg 9.0.2（LGPL 構成・配布 runtime と同一）/ release build / revision `d855073` + レビュー修正（`execute_with_inputs`・`SURFACE_BUDGET_EXCEEDED`）。fixture `hevc8k.mov` は 7680×4320 HEVC yuv420p tv/bt709・24 fps・48 kHz PCM・240 フレーム・141,054,431 bytes。n=24 フレーム/ケース。

`redraw_total_ms` は GUI が 1 フレームに実行する全段（`preview_dag_unresolved` + `resolve_dag_resident` + `preview_texture_resident` + `gpu.wait`）の合計。

| ケース | redraw p50 (ms) | 最大 (ms) | 予算 41.7ms |
|---|---:|---:|---|
| 8K → 1512x851, effect なし | 18.14 | 609.9 (初回解決) | 内 |
| 8K → 1512x851, Gaussian blur | 18.45 | 366.4 (初回解決) | 内 |
| 8K → 1920x1080, Gaussian blur | 19.82 | 371.7 (初回解決) | 内 |

参照系の `fresh_decode_video_image_ms`（旧経路と同じ「runtime 再ロード＋decoder 再オープン＋全量 SHA-256＋RGBA f32 全展開」を毎フレーム実行する API）は同 fixture で p50 **636.9ms** であり、resident 経路との比較で約 34 倍の差がある。decoder pool 統計は hits 23 / misses 1 / evictions 0 / retained 99,532,800 bytes（8K yuv420p 1 フレーム×2 保持）で、プールがフレーム間で decoder を再利用し保持上限 512 MiB 内に収まることを確認した。

初回フレームの max ~370-610ms は runtime ロード・decoder 初回オープン・初回全量 hash・GPU パイプラインコンパイルを含む cold コストであり、連続再生の steady-state を表さない。

- [8K→1512x851 none raw JSON](perf-001-resident-preview-measurements-8k-1512x851-none.json)
- [8K→1512x851 blur raw JSON](perf-001-resident-preview-measurements-8k-1512x851-blur.json)
- [8K→1920x1080 blur raw JSON](perf-001-resident-preview-measurements-8k-1920x1080-blur.json)

## GUI 実機検証

Computer Use による macOS アプリ検証（2026-10-09）でも、同 8K シーケンスで Gaussian Blur を適用した状態で 10 秒クリップが実時間完走・`underrun 0`・停止時刻がクリップ終端と一致することを確認した。音声 underrun の指標は音声 callback の実測欠落カウントであり、映像見出しの p50 とは別契約である。

## CPU/GPU 等価

`kronello-service` の `service_software_upload_resident_matches_software_preview` テストが、resident upload 経路と明示的ソフトウェア経路のピクセル一致を確認する（閾値 max < 0.02、実測 ~0.0088）。YUV420P 経路の chroma nearest サンプリングとソフトウェア bilinear アップサンプルの差は ADR-0140 の既知制限として記録済み。
