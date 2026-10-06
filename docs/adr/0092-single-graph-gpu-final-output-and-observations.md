# ADR-0092: GPU graph の単一実行と要求単位の観測

- 状態: 採用
- 日付: 2026-10-06

## 決定

最終 linear / display 出力を同じ GPU graph の最終 RGBA16F texture から生成する。display の変換順・色・alpha 契約は変更しない。両面を生成した後に sticky validation を一度確認し、成功した graph の node 面だけを cache へ公開する。linear / display の読み戻しは明示した最終出力境界だけで行い、linear 出力専用の中間 texture copy を不要にする。strict resident decode も同じ graph を使い、CPU 中間往復を加えない。

`RenderBackend::transfer_stats_total` は backend の単調累積実行観測を提供する。共有 frame / streaming / temporal の入口・出口で差分を取り、全 tile / sample の transfer を要求単位で返す。cache hit で backend が実行されなければ差分はゼロ。既存 injected backend は opt-in しない限り従来の `transfer_stats` 契約を保持する。共有 GPU context の通常要求は thread-owned reentrant scope で直列化する。同じ thread の tile / sample / backend 入れ子は許可し、独立した thread は Condvar で最大 30 秒待つ。期限超過は `RENDER_BACKEND_BUSY`。公開 `try_observation_scope` / `try_execute` は明示 nonblocking で同じ typed code を返す。token は `!Send`。scope は baseline 読取前、native decode / upload 前に取得し、preview / readback / resource 設定も守る。異なる context は独立して実行できる。

`gpu_wait_operations` は renderer の明示 wgpu completion poll、`gpu_compute_dispatches` は実 dispatch を数える。native decoder callback の待機や preview consumer の追加 fence は別の観測とする。既存 upload / copy / row-padded readback の意味は維持する。

GPU allocation の観測は、具体的な descriptor と保持した resource handle に基づく payload ledger とする。graph / cache texture、control buffer、readback buffer、sampled resident 入力、旧出力 copy を分類し、clone では重複加算せず、最後の所有者で減算する。resident 面の cache lease も元の同じ allocation guard を共有し、scene 消滅後も cache eviction まで観測を保持する。node の入れ子実行中の owned payload peak を記録する。idle pool は別に表示し、cache bytes は graph 所有の部分集合なので二重加算しない。driver alignment / private allocation、native decoder pool、外部 raw texture の消費側寿命を物理 GPU memory の既知値として扱わない。CPU glyph / temporal accumulator と FFmpeg encoder の memory は process footprint 側で区別する。

## 検証

cache を切った従来の二 graph と単一 graph の全画素厳密一致、tile / temporal 転送集計、cache hit のゼロ転送、所有権と分類別 peak を actual GPU で検証する。速度比較は release binary、固定入力、cold / warm 分離、各 21 sample と quiet host window を使う。測定結果は [PERF-001](../testing/perf-001.md) の正本へ統合する。

最終 output の直前に compiler が付加する synthetic root が単一子・opacity 1 の場合に限り、GPU lowering でその子への参照へ置換する。内部 group の一般的 flatten は行わない。最終 SourceOver/store の正規化・validation を維持し、対応する GPU semantic key 配列だけ同じ root を除く。direct/ mask/blur、alpha edge、両 working space と出力 space/alpha の旧経路との bit identity を実 GPU で検証する。surface budget は維持し、限定省略の対象外は従来どおり typed failure とする。
