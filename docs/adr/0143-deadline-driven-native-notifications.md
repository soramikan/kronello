# ADR-0143: native の外部状態確認を描画頻度から分離する

- 状態: 採用
- 日付: 2026-10-11
- 対象: PERF-002
- 部分置換: ADR-0056 の work 完了ごとの外部状態確認。FIFO・通知の JSON・集約・安全モード排他を維持する。

## 背景

購読中に要求完了ごとに `project.info` と `job.list` を実行すると、文書の読み取り・検証・ハッシュ計算が次の描画要求を待たせる。release native の連続した軽い要求で、文書規模に比例する待ちを確認した。

## 決定

- 外部状態の定期確認は前回の確認終了から250ms後を期限とする。描画・読み取り query・失敗した要求は期限を変更しない。
- worker の受信待ちは期限までの時間とし、要求が続いても期限到達時に確認する。
- 購読開始と、文書変更またはジョブ状態変更を伴う成功した共有 API 呼び出しの後は即時確認する。
- `revision_changed` / `job_progress` は従来と同じ共有 Response と最新値への集約を維持する。編集結果の completion を通知確認より先に公開する。
- 定期確認を実行する間の費用自体は残る。revision 単位の準備再利用は PERF-003 で別途計測する。
- macOS app の `--release` は Rust FFI/CLI と Swift build/bin path の両方で release を選択する。

## 検証

`scripts/measure_ffi_polling.py` で100/1000/5000クリップの連続要求を購読あり/なしで比較する。これは GPU 描画・GUI 表示を含まない worker turnaround であり FPS 保証ではない。外部編集・job通知は FFI の境界テストで確認する。実行結果は [PERF-002](../testing/perf-002.md) に記録する。
