# ADR-0140: メディアセッションの検証メモ化とプレビューの GPU resident デコード

- 状態: 採用
- 日付: 2026-10-09
- 対象: PERF-001 の preview/export 実行時経路
- 部分置換: [ADR-0091](0091-exact-forward-decoder-and-bounded-render-scope.md) の「区間命中時も毎回ファイルの hash を検証する」条項と保持 byte budget のみ

## 背景

プレビューの描画は `preview_dag` がフレーム毎に decoder を開き直し、`resolve_asset` が資産全体の SHA-256 を毎アクセス再計算していた。25 MB 級の素材でも 1 フレームに複数回の全量 hash が走り、8K ではさらに 16 MP のピクセル budget が型付きエラーで要求を拒否した。計測では `preview_dag` が p50 565 ms を要し、等倍再生の 41.7 ms 予算に対して桁違いに遅かった。

## 決定

### 実行時検証のメモ化

`assets::resolve_asset` は従来どおり一発呼び出しで常に全量 hash を検証する非メモ化の入口として残す。`MediaRuntime` に実行時スコープの検証メモを追加し、レンダー経路は `resolve_verified` を呼ぶ。

- キーは canonical path。値は `FileFingerprint`（size、mtime、ctime、inode/file index 相当のメタデータ）。
- `locate_asset` の stat 情報と記録済み fingerprint が一致した場合のみ、完了済みの全量 hash 検証を再利用する。size・mtime・ctime・inode のいずれかが変化した場合は全量 SHA-256 を再計算する。
- メモは `Rc` で共有される `MediaRuntime` のクローン間（`MediaSession` とそれを使う backend）で有効で、プロセスグローバルではない。runtime を load し直した新しいコンテキストは必ず初回に全量 hash を取る。

この緩和で残る脅威は「size・mtime・ctime・inode をすべて保存した書き換え」（権限を持つローカルプロセスによる細工）が同一 runtime の寿命内で検出を免れる場合に限られる。受け入れ根拠は、(a) 素材はローカルの大容量ファイルで通常の編集・置換・再 mux は mtime/ctime/inode のいずれかを必ず変える、(b) メモ化は runtime 寿命に限定され import/publish 等の一発経路は依然全量検証、(c) 検出不能な書き換えはすでに同一プロセス権限の行為であり OS 境界上の追加防御を提供しない、の3点。

### 永続 decoder pool

`MediaSession` が `(canonical path, stream index, asset)` をキーに最大 2 個の `VideoDecoder` を LRU 保持する。呼び出しをまたいだ保持 native planes の合計上限を 128 MiB から 512 MiB に引き上げ、8K の current/lookahead 2 フレームが常に破棄されないようにする。順方向デコード・exact restart・統計の意味は ADR-0091 のまま変更しない。

### ネイティブ shim のスレッド化

`media.c` のビデオ decoder コンテキストで `thread_count=1` の固定をやめ、FFmpeg の自動スレッド選択を許す。スケジューリングのみが変わり出力ビット列は不変。`sws_alloc_context`/`sws_init_context`/`av_opt_set_int` を動的ロードし、`video_rgba`/`video_rgba64` の swscale コンテキストは AVOption で `threads=0`（自動）を指定してから初期化する。FFmpeg バージョン間で公開フィールドの有無が異なるため直接フィールド代入はしない。

### 8K ピクセル budget

`VideoDecoder` のデコード拒否上限を 16,777,216 px / 268,435,456 B から 67,108,864 px / 536,870,912 B に引き上げ、8K DCI (8192×4320=35.4 MP) の RGBA64 展開まで収める。上限超過は引き続き `UnsupportedFeature` の型付きエラーで、黙った縮小や fallback はしない。

### プレビューの GPU resident デコード

ネイティブプレビュー（FFI `preview.render_frame`）は DAG を未解決のまま構築し、budget-fit の再試行はサーフェス見積もりだけを繰り返す。受け入れた出力サイズで `resolve_dag_media_resident` が各 `VideoDraw` ノードを振り分ける。

- 適格ノード（順方向・補間なし・HDR なし・smart-reframe crop なし・LinearRec709/2020 working space）は `VideoResolution::Deferred` のまま残し、pooled decoder が提示したフレームを `GpuContext::sample_upload_to_working` でテクスチャへ送る。BT.709 tv-range の `yuv420p` は Y 平面と U/V インターリーブ RG8 の planar upload、その他は既存の swscale RGBA8 を upload する。共通シェーダー `resident_media.wgsl` が出力解像度でサンプリング・逆伝達・working space 行列・premultiply を実行する。
- 不適格ノードと画像・カメラ RAW は従来の明示的ソフトウェア経路（`RasterInput`/`VideoSource`）へノード単位で戻す。fallback は同一意味の実装差であり黙った品質変更ではない。
- キャッシュ識別は decoder 実装バージョン・資産 content hash・stream index・時刻・提示フレームの PTS/end・format/color タグ・シェーダー内容 hash を含む `input_identities` で、`RasterCacheKey::external_source` 経由で既存のキー体系に乗る。

VideoToolbox 常駐経路（`require_gpu_resident`）とこの upload 経路は同じシェーダーと identity 規約を共有し、色契約を二重化しない。

## 検証

- `perf_video_preview`（release）で 8K HEVC・blur あり・1512x851/1920x1080 出力の p50 フレーム時間と decoder pool 統計を測定する。
- `service_software_upload_resident_matches_software_preview` が upload 経路と明示的ソフトウェア経路のピクセル一致（max < 0.02、実測 ~0.0088）を確認する。
- 既存の media seek 検証（perf-001）と GPU resident 検証（gpu_resident）の契約は維持する。
