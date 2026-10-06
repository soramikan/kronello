# ADR-0089: 容量制限付き GPU texture と project 外 raster cache

- 状態: 採用
- 日付: 2026-10-06

## 決定

CPU の `RasterCacheKey` と GPU node cache は同じ意味キーを使う。キー版 `cache003-json-sha256-v2` は時刻、入力 hash / stream、ROI / halo、作業用色空間、coverage / effect 意味版、backend namespace を含む。空 composite も ROI / 色 / backend を固定する。動画の未解決入力はキー生成で拒否する。

GPU cache は実際の RGBA16F texture を LRU で保持する。texture LRU と同寸法・usage の idle texture pool は独立した entry / byte 上限を持つ。clone された lease は最後の所有者が解放するまで pool に戻さない。同一 queue の順序を利用し、context identity を独立 token で検証する。公開 preview texture へ切り離した面は再利用しない。外部 resident texture の context / dimensions / 色は、親の cache hit より前に検証する。GPU node の global LRU 公開は graph 全体の sticky validation 成功後に限る。失敗した graph は、後続の opaque 描画に隠れた中間面も cache へ公開しない。

任意の永続 disk cache は project 外に置く。既定は OS の user cache directory。広い project 親（HOME など）が既定 cache を含む場合は disk のみ無効化し、`default_directory_overlaps_project` を返す。明示 `KRONELLO_RASTER_CACHE_ROOT` が project 内なら `CACHE_CONFIGURATION` とする。`KRONELLO_RASTER_CACHE_DISABLE=1` は `explicitly_disabled` を返す。

永続面には意味キー、adapter / API / device / driver、OS build、wgpu / Naga 版、shader / renderer source と lockfile の fingerprint、寸法、payload checksum を固定する。OS fingerprint を取得できなければ永続化を拒否する。入力の不一致・破損・非有限値は miss として再計算し、観測値に記録する。真の filesystem I/O failure は `CACHE_IO` とする。

publication は同 directory の同期済み一時 file を hard link で no-replace 公開する。同時 writer の有効な winner を検証し、peer eviction の `NotFound` は miss / 再試行として扱う。read は期待サイズ + 1 byte で制限し、eviction 探索の保持件数も制限する。disk entry 上限は 4096。cache 削除・容量 eviction は描画意味を変えない。

通常 GPU の disk 書込は実 GPU readback、hit は実 GPU upload として転送を記録する。strict GPU-resident 動画と texture preview では disk 経路を使わない。strict 経路に CPU 中間往復を加えない。resource cache counters は context 累積の診断で、作品の意味的同一性には含めない。

## 検証

実 texture、clone / context 寿命、ROI / halo、preview、破損、削除、同時 process、project 外境界の実機検証は [CACHE-003](../testing/cache-003.md) を参照する。GUI の持続 `GpuContext` は texture / pool を再利用し、通常 service の独立要求は新しい context なので disk の共有を検証する。この二つの warm 経路を混同しない。
