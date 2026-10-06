# CACHE-003 実機検証

## 実装と経路

[ADR-0089](../adr/0089-budgeted-gpu-and-external-raster-cache.md) の共有意味キー、GPU texture LRU、lease pool、project 外 disk raster cache を実装した。default texture / pool は各 256 entries / 64 MiB。disk は設定された容量で制限する。`FrameMetadata.resource_cache_stats` は actual entry / byte / hit / miss / eviction と persistent policy、破損拒否数を表す。転送は `transfer_stats` に実 upload / copy / readback（Metal row padding を含む）を記録する。

strict resident 動画は texture / pool のみを使い、disk policy は `disabled_for_gpu_resident`。sample decode / 色 / device 検証は cache hit でも省略しない。通常 service の新規要求は新規 GPU context で、同 process の texture warm と異なる。GUI preview の既存持続 context は texture warm を利用し、CPU disk 往復を行わない。

## 再現コマンド

```sh
cargo test -p kronello-gpu resource_cache --locked -- --ignored --nocapture
cargo test -p kronello-gpu --test resource_cache --locked -- --ignored --nocapture
cargo test -p kronello-service --test gpu_cache cache003_actual_service_external_disk_fresh_context_and_deletion --locked -- --ignored --nocapture
cargo test -p kronello-render --test render gpu_animated_shape_and_japanese_text_match_cpu_all_pixels_and_order --locked -- --nocapture
```

2026-10-06、Apple M4 / Mac mini 32 GB / macOS 27.0.1 (26A434)、Metal IntegratedGpu の dirty workspace（基点 `4b75d40`）で実行。性能時間の比較は並行作業中のため採用しない。最終 clean checkpoint での再検証は別記する。

GPU unit 3 件が合格。実 texture LRU と pool の capacity、clone の早期再利用禁止、context / 寸法拒否、context を越える lease 寿命、raw preview 切離し、disk 削除、checksum / fingerprint mismatch、typed I/O failure、OS fingerprint 不足拒否を確認した。8×8 面の cold は pixel upload 512 B / control 1252 B / copy 512 B / readback 6148 B、同 context warm は upload 0 B / control 420 B / copy 512 B / readback 2052 B。fresh context disk hit は pixel upload 512 B / control 420 B / readback 2052 B。全画素は削除前後も一致する。disk 1-entry / 1024 B の試験は実 entry 1 / 624 B に収まり 5 回 eviction、破損等の拒否は 4 件。

service 親テスト 1 件と 5 child process が合格。fresh context の disk hit、外部 cache 削除後の全 linear / display 画素一致、同じ cache への 2 process × 8 要求の競合、明示 project 内 directory 拒否、synthetic HOME 親での既定 disk 無効化と render 成功を確認した。actual HOME に project を作成しない。

animated shape / 日本語 text の CPU 全画素 oracle と逆順 GPU 要求の意味的同一性テスト 1 件が合格。transfer / cache 診断だけを除外し、全画素と他の metadata は厳密比較した。

DAG effect / tile halo / preview 1 件が合格。24 px blur の full / 2 tiles は全画素最大差 0、cold / warm の linear / display も厳密一致。control upload は 3780 B / 28 operations から 1256 B / 11 operations に減り、preview の持続 context hit と保持中の texture の非再利用も確認した。証拠は `target/evidence/cache-003/` に保存する。cache counters は context 累積値であり、render frame の意味値ではない。複数 tile / temporal sample にまたがる transfer 集計と単一 graph での linear / display 出力最適化は PERF-001 で検証する。

GPU-003 回帰は production service 4 件と native resident 2 件が合格。actual compressed AVC high-profile extension の 8-bit / 4:2:0 確認、CFR / VFR / B-frame の exact PTS、PQ / 10-bit / full-range / sRGB forged lock 拒否を維持した。scoped `cargo clippy -p kronello-gpu -p kronello-service --all-targets --locked -- -D warnings` も合格。

失敗 graph の cache 汚染回帰も actual GPU で合格。有限入力の合成 overflow を opaque な後続描画で隠す scene を full / preview の各経路で 2 回要求し、毎回 typed error、global texture inserts / entries が 0 のままであることを検証する。node 面の保持は全 graph の sticky validation 成功後まで延期する。
