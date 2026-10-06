# PERF-001 GPU 出力統合と観測

## 経路と再現

[ADR-0092](../adr/0092-single-graph-gpu-final-output-and-observations.md) の単一 graph 出力を通常 backend / strict resident backend に統合した。最終 linear / display の色・alpha 契約を保ち、sticky status 確認を一度にし、linear 専用の copy texture を除いた。native texture preview は最終画像の CPU readback を行わず、DAG の execution halo を保持する。GUI は `crop_origin` を presentation uniform に渡す。final 画素と比較する native preview oracle は同じ crop を適用する。

```sh
cargo test -p kronello-gpu perf001_fused_graph_matches_doublepass_and_tracks_ownership --locked -- --ignored --nocapture
cargo test -p kronello-gpu --test resource_cache perf001_request_transfers_sum_tiles_samples_and_zero_work_hits --locked -- --ignored --nocapture
cargo test -p kronello-gpu --release perf001_gpu_fusion_measurement --locked -- --ignored --nocapture
```

全 sample の保存記録は [測定 JSON](perf-001-gpu-fusion-measurements.json)。再実行時の出力先は `target/evidence/perf-001/gpu-fusion.json`。release / 21 sample、proxy 320×180 / preview 640×360 / full 1920×1080 の fixed raster + blur + isolated opacity scene を使う。legacy の二 graph と fused graph を同じ binary に保持し、cold は texture LRU / pool / disk を無効化して同じ context 上で比較する。device 初期化時間や driver 内部 cache を「cold」の意味に含めない。warm は実 texture LRU / pool を準備し別ケースで比較する。disk は全ケースで無効。全 sample の linear / display 画素 SHA-256 が厳密一致する。JSON は実行 revision / dirty、compile-time source と実 binary の SHA-256、各 sample の実転送・wait・dispatch・allocation を保存する。

## 実機の正しさ

2026-10-06 Apple M4 / Metal の dirty workspace（基点 `4b75d40`）で fusion / 要求集計 / 並行性 / resident 寿命 / typed API の correctness 5 件が合格。64×36 scene の全 linear / display 画素は二 graph と厳密一致した。pixel upload 36864→18432 B、control upload 3320→1868 B、texture copy 18432→0 B、readback 36872→36868 B、completion wait 4→3、compute dispatch 11→6。並行作業中の correctness run の経過時間を性能値として採用しない。

二 tile の frame / streaming metadata は各 6 readback / wait を報告し、最後の tile の 3 回だけを返さない。三 temporal sample は各 sample の転送を合計し、同じ temporal cache からの再要求は transfer 0。入口・出口の backend 累積差分を使うため、同 context の以前の frame の転送を新しい frame に含めない。

## memory の観測範囲

`GpuContext::allocation_stats` は実 resource handle の owned descriptor payload を記録する。graph / cache texture は RGBA16F 寸法×8 B、control / readback buffer は実 `Buffer::size`（row padding を含む）、sampled resident 入力と旧出力 copy は独立分類。clone は一度だけ数え、最後の lease で減算する。control buffer は completion まで実 handle を保持して観測する。node 内の入れ子実行中の owned payload peak と、全経路の同時 owned payload peak を返す。idle pool は別欄、cache byte は graph 面の部分集合なので合算しない。

correctness の fused 64×36 scene は owned payload peak 130892 B、graph 面 peak 110592 B（6 resources）、control peak 1868 B、readback peak 18432 B。出力 copy は 0。cache / pool を切った処理後は live owned payload 0。lease clone の片方を解放しても 512 B を保持し、最後の解放で 0 になることを確認した。

これらは driver 物理 allocation の測定値ではない。alignment / compression / pipeline private allocation、VideoToolbox decoder pool、外部へ返した raw preview texture の消費側寿命は未知。CPU glyph / temporal accumulation と FFmpeg encoder は GPU atlas / accumulator / encoder memory と呼ばず、overall harness の process RSS / footprint と別に観測する。将来の GPU memory 推定と混同しない。

## 受け入れ測定

実 Apple M4 / Metal、macOS 27.0.1 (26A434)、32 GB の root 検証・GUI block 後、他 agent の build / test / measurement を停止した exclusive quiet window で release harness を実行した。12 cases × 21 samples = 252 samples が完走し、旧二 graph と fused の linear / display SHA-256 は全 sample で厳密一致した。p95 は 21 個を昇順にした 20 番目（nearest-rank）。所要時間は同期 readback 完了までを含み、device 起動・scene 構築・hash 計算は含まない。

| case | legacy median / p95 (ms) | fused median / p95 (ms) | median 短縮 |
|---|---:|---:|---:|
| proxy cold | 9.383 / 11.165 | 5.599 / 6.607 | 40.3% |
| proxy warm | 5.127 / 5.585 | 3.255 / 3.857 | 36.5% |
| preview cold | 19.465 / 21.875 | 12.900 / 13.785 | 33.7% |
| preview warm | 9.580 / 11.064 | 7.578 / 8.223 | 20.9% |
| full cold | 124.243 / 127.669 | 80.461 / 81.946 | 35.2% |
| full warm | 47.110 / 47.768 | 41.767 / 42.834 | 11.3% |

cold の全サイズで upload は 2→1、compute dispatch は 11→6、GPU copy は 1→0、readback は 4→3、GPU wait は 4→3。warm では upload は双方 0、dispatch は 3→2、copy は 1→0、readback / wait は同じ 4→3。1080p cold の pixel upload は 33,177,600→16,588,800 bytes、copy は 16,588,800→0 bytes、readback は 33,177,608→33,177,604 bytes。control upload は cold 3,320→1,868 bytes、warm 1,256→836 bytes。readback の差 4 bytes は sticky validation buffer の一回分であり、二出力の画素 readback 自体は省略しない。

tracked peak owned payload は旧/新で等しく、proxy cold 3,227,468 bytes / warm 3,226,436 bytes、preview cold 12,904,268 / warm 12,903,236 bytes、full cold 116,123,468 / warm 116,122,436 bytes。cold の終了時 live owned は 0、warm の retained live は proxy 1,382,400 / preview 5,529,600 / full 49,766,400 bytes。fusion が memory peak を削減したとは主張しない。driver private / native decoder pool は unknown のまま、per-node peaks と各 resource category は raw JSON を参照する。この scene は CPU raster + blur + opacity なので native decode / encoder の allocation を含まない。

provenance は dirty `4b75d40d452538112a34b69eff94195d41242d4e` 上の凍結 source、compile-time source bundle SHA-256 `ba44edb43f41f82f82e461a25f0280ce2d5863927f72a397926c0a883cea6669`、実 release test binary SHA-256 `d3ef47d0d67df8e86af47508316081cdfc6e034c92a18be85a7c4e740e9c58e2`。source bundle は renderer.rs / scene_gpu.rs / allocation.rs / perf_tests.rs / Cargo.lock の連結であり、全 repository の hash ではない。測定時の exact source と binary を特定する記録であり、clean HEAD 測定とは呼ばない。実行 log は `target/evidence/perf-001/gpu-fusion-release.log`。JSON は dirty revision、adapter、各 sample の counters / resource categories / node peaks を保持する。

overall preview / final / history / native decode の測定は [PERF-001](perf-001.md) と併せて判断する。

共有 GPU context の concurrent 2-thread 要求は、owner thread の nested render を許可しつつ peer を待機させ、両要求が成功した。2 tile 要求は 6、peer の 1 tile 要求は 3、context 合計は 9 readback。別要求の counters を metadata に混入しない。明示 nonblocking execute の `RENDER_BACKEND_BUSY` は actual ownership 競合から生成し、shared `ServiceError` / JSON の code として維持し、owner 解放後の通常 execute は成功した。通常経路は Condvar の最大 30 秒待機で直列化し、timeout も同じ typed code。token は別 thread へ移動できない。

resident cache の ownership 回帰では scene / ResidentImage 解放後も cache が同じ allocation guard を保持し 512 B を報告、eviction 後 0 B に戻る。temporal の最終 `resource_cache_stats` は最後の sample 後の実 backend 値と一致する。final cache native 3 件と strict resident production 4 件の回帰、scoped GPU / service all-target clippy も合格した。

複雑な lower-third scene の 1080p / 4K whole-DAG native preview は固定 512 MiB の保守的 scene admission で typed unsupported とする。拒否前に graph 面は割り当てないため、admission estimate を actual allocated peak と呼ばない。native preview tiling は現在の API に含めず、cap を拡大して速度を主張しない。overall harness は proxy native preview、1080p / 4K の拒否、tiled final を区別する。

### 最終 output root の限定省略

GPU lowering は、DAG の末尾 `OutputTransform` が直前の `IsolatedComposite` を参照し、その子が 1 個かつ opacity が厳密に 1 の場合だけ synthetic root を子への参照に置換する。内部 group、複数の子、非単位 opacity は省略しない。semantic cache key の配列も同じ root だけを除く。最終 scene composite の SourceOver と RGBA16F store は残るため、透明画素の正規化、出力の straight/premultiplied 変換、sticky numeric validation は継続する。全 resident input の device/format/ownership 検証も cache lookup より前に継続する。

`perf001_singleton_output_root_elision_is_bit_exact` は direct child と mask/blur child の両方について LinearRec709/LinearRec2020、sRGB/linear709/linear2020、straight/premultiplied を旧 root 有りと全画素 bit 比較する。signed zero、最小付近の正 alpha、負 RGB、HDR 値を含む。非ゼロ RGB の alpha 0 CPU raster は既存契約で無効なので、旧/新の両方が同じ `InvalidInput("invalid raster input")` を返すことも確認する。実 M4 で 1 passed。lowering/cache key alignment の通常テスト 1 passed。`perf001_basic_4k_preview_admits_elided_root` は実 M4 の 3840×2160 native texture 生成を確認し 1 passed。単純な既存 fixture の保守的 admission は 11 surfaces から 8 surfaces（530,841,600 bytes）へ減る。512 MiB cap は変更しない。複雑な 4K scene の typed admission failure は残り、これを realtime 成功とは扱わない。

### 6384858 後の bounded coverage skip

33.3 ms の目標を維持し、solid fill の有限な outline AABB の外側だけ coverage sample loop を省略する。design 座標に f32 誤差の guard と 2 pixel 分の guard を加える。各 pixel の RGBA zero store は残すため、pool の前フレーム内容を残さない。stroke / local stroke geometry / gradient / 非有限・極端な座標や scale / 空 outline は旧 loop に戻す。dispatch 範囲、cache key、sticky validation、ownership、512 MiB budget は変えない。既存 uniform の solid fill では使わない `fill_extra` を bound として使い、control upload bytes は増やさない。

実 M4 の `gpu_coverage_bounds_*` は 4 passed / 0 failed / 0 skipped。同一 shader の bound 有効/無効を全 linear/display bit 比較し、fractional な斜辺、部分 clip / 全画面外、alpha 0 / 最小付近 / 0.5 / 1、stroke / gradient fallback、cold / warm pool、anisotropic scale、64/3840 × 32/2160 の実 4K 相当 scale、負の resolved 座標、Evenodd の複数 contour と implicit close を含む。実 texture cache hit も旧 loop と厳密一致した。RGBA16F overflow は transparent / opaque parent に隠れても両経路で繰り返し typed failure となり、GPU cache insert は 0 のまま。既存 actual GPU scene 28 tests と vec004 9 tests も全て pass / 0 skip。通常の uncertain bound fallback unit test、GPU scoped clippy、workspace fmt を確認する。log は `target/evidence/perf-001/coverage-bounds-*.log`。これらは correctness evidence であり、最適化後の 4K realtime 達成を示す timing evidence とは扱わない。最適化後 frozen source の 21 animated sample は overall harness で別に測定する。
