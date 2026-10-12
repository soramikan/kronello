# 書き出し性能改善計画の完了監査

対象は[依頼計画](performance-export-plan.md)、基点はmain `656ecc3`。2026-10-13時点の実装・受け入れ証拠を条件別に対応付ける。過去の段階的記録の「未完了」を消さず、この対応表と各記録の最終受け入れ節を現在の範囲とする。PR #21のCI/再レビューの確定までは最終delivery gateを完了扱いにしない。

## 条件と現在の証拠

| 計画 | 実装・測定・検証 |
|---|---|
| PERF-006: 固定fixture/identity/binary・実worker全工程 | `execution.rs`の診断、`perf_export_worker.py`のrender.submit→succeeded、[006](perf-006.md)の8case・異開始点/速度・長尺。全工程/待ち/DB commit/変換/保持payloadとRSSを別fieldで記録 |
| 過去codec資料の不整合 | [006](perf-006.md)の訂正と固定source/fixture・異なるbinaryでの108pair再採取。旧不一致を成功へ上書きしない |
| PERF-007: 静的準備/hash/font/LUT・sample SceneIR | `ExportRenderSession`、検証済みsnapshot/font、16件/64MiB scene cache。[007](perf-007.md)の3/8 samples、4096 samples上限、cut/ROI/halo/f64加算順・日本語/HDR全画素bit・missing/hash mismatch・sink失敗。3sample実workerでSceneIR121→3、評価hash121→1、static preparation1 |
| PERF-008: 素材frame/packed共有・SwsContext・独立cursor | immutable Arc native frameと変換前cache、8件/256MiB・SwsContext2slot。[008](perf-008.md)の4K変換40→1、3/8 samples120/320→2、全画素/PTS/end、reverse/HDR/flow・source書換え/期待hash拒否。logical保持payloadとsampled RSSを区別 |
| PERF-009: 経過時間での進捗persist・制御 | `WorkerProgress`の確認50ms/進捗200ms、100/200/250ms比較、同値抑制、独立heartbeat/final fence。[009](perf-009.md)のcommit数、cancel/lease/競合/kill/resume。audio200msのraw一pairではcheckpoint7046→1、全write commit7051→4。native停止時間を確認間隔と混同しない |
| PERF-010: 直接変換・同条件出力共有 | `packed_into`のchecked stride/offset、従来f32/f64境界、owner二組/同条件video共有。[010](perf-010.md)の19比較114workers、別背景/HDR/別codec・音声/chapters/hash/priming維持。SWS1/2/4比較は改善なしで1、ProRes4/DNx2/SVT lp1を維持。encoder待ち支配を観測せずleg別queueは追加しない |
| PERF-011: mix/量子化共有・長尺source準備 | ordered plan/意味版/lock/range/layout/gain/effects/clippingのgroup、4096 quantum/有界codec adapter。[011](perf-011.md)の72workers、全sample/partial/layout/clipping/WSOLA/DSP/priming/PTS。検証済みspool8件/2GiBは任意policy、source全文hash・参照window checksum・競合/破損/取消/回収を確認。cold費用・競合p95を根拠に既定cache/同一job A/V並列化/range decodeは未採用 |
| PERF-012: status/画像wait統合・有界staging | sticky status/画像copyとmapの一完了単位、managed4件/16MiB、成功完了/unmap後のみ再利用。[012](perf-012.md)の実Metal、4K wait80→40・3sample240→120、NaN/sticky/失敗/未使用/map待機Drop・24frame持続処理。transfer bytes不変、CPU packing overlapは新設しない |
| PERF-013: 共通header/chunk/root receipt・page・復旧 | header8MiB、64件/4MiB chunk、逐次index、lazy有理数frame plan、summaryとjob.frames64件/16MiB。[013](perf-013.md)のmovie/GIF/連番1000/10000frame、全semantic metadata/両payload復元、worker RSS/別Query RSS、旧reader/receipt、欠落/破損/順序/extra-file、cancel/kill/rename/DB境界/reconcile/prune |
| 確定工程のprobe共有・native mux取消 | private probe2件再利用、64packet checkpoint/catch_unwind、最終probe・PTS/DTS/duration/priming/chapters/hash/NOREPLACE。[013](perf-013.md)のpacket/全file比較と非空chapters試験。短いmuxで全体速度改善は未確認 |
| PERF-014: 対象backend/formatの試作・全体時間/色/時刻/寿命 | [014](perf-014.md)の限定SDR Metal→BGRA IOSurface→hardware H.264/MOV、safe mathと同一f32 LUT、GPU/VT完了前のreuse禁止、異常/取消/UTF-8/既存file/stage回収。4K交互9pair、全decoded画素・YUV420/tv/BT.709・全432packetの正確なPTS/DTS/duration。全体warm p50 954.527→632.037ms、p95 993.198→643.842ms |
| PERF-014: packet直接muxと採否 | 生成silence PCM24/SDR ProRes/MOVの9pair、全PCM/画素/time/tag/packet/file bytes一致、全体warm p50 1208.954→1176.189ms、中間video42,079,665bytes省略。progressive fiel差を修正。両試作は任意featureのみ、[ADR-0167](../adr/0167-experimental-export-delivery.md)で標準採用保留。renderer/worker統合・他matte/HDR/他OS・実音声/nonempty chapters/priming/multilegへ保証を拡大しない |
| レビューで検出したpublication回帰 | [ADR-0168](../adr/0168-publication-root-compatibility.md)：9root独立制限撤去、非空root≤既存全file budget、12root公開/復旧。旧attempt receipt/index両回収、孤立index・無関係file保護。jobs全feature36pass/0fail/1ignored、recovery10件、全target/all-feature Clippy |

## 原記録と保証の区別

完了監査でPERF-006〜012のチェックイン済み要約が参照するraw evidence 60件を読み、byte長とSHA-256を再照合した。全634 fixed-worker trialsの`worker.succeeded`、snapshot identityと全decoded bits/PTS/layoutの比較成功を確認した。監査出力は`target/perf-completion-raw-audit.json`。圧縮file/packet差（MXF/WebM/VT private SEI）、静穏でないhost、少ない後続試行、first fresh workerと同process warm、header/poolのlogical payloadとRSSの差を各記録から除かない。

最終通常構成（`b9fdea30`）はfmt、workspace/all-targets/all-features Clippyとworkspace testsを通過。testsは202 suites / 1379 passed / 0 failed / 50 ignored / exit0、log SHA-256 `7ea09098a820a6cff96258756e038e5dc565b77df87a5ef71425fb2cbd8f139d`。その後の局所レビュー修正（`8ca642b2`）は上表のjobs全機能・実processで検証した。初期all-feature 1391passの結果と、現在のMOV版3件の別検証を混ぜない。raw statusと最終gate hashは`target/perf-014-final-validation.json`、PR本文にも保存した。

macOS初回CIのMCP timeoutは[debug SHA検証](mcp-debug-hash-latency.md)で対処し、修正後のmacOS CI成功を確認した。再レビューの`Stale file handle`は環境エラーで、コード指摘解消の証拠とはしない。最新headのCI/再レビューは別途確認する。UI viewport対応は計画末尾の次候補であり、今回の書き出し改善の完了項目へ加えない。
