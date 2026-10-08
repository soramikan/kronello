# ADR-0128: ソース/プログラム二画面モニタと三点編集

- 状態: 採用
- 日付: 2026-10-08

## 背景

GUI-011 はソースモニタとプログラムモニタの二画面編集 UI と
insert/overwrite 編集操作の接続を要求する。パネル構成と
編集意味を共有 API と揃えて決める。

## 決定

- Edit ページに SourceMonitor（asset プレビュー、in/out
  設定、挿入先映像・音声の表示）と既存 Program モニタの
  二画面を実装する。SourceMonitor の開く対象は Project
  panel / bin の asset と timeline クリップの両方とする。
- 三点編集は `edit.insert` と `edit.overwrite` の共有編集
  API とする。source in/out と sequence の in/out・playhead、
  `Sequence.targets`（NLE-005）を組み合わせ、三点が決まれば
  残り一点を導出する。競合・ロック・対象欠落は既存の
  型付きエラーを再利用する。
- insert は後続を ripple させ、overwrite は被覆を上書き
  する。両者とも Event として記録し Undo・revision 競合は
  既存規則と同じ。
- GUI には insert / overwrite ボタンとショートカットを
  追加し、ワークフロー設定（FLOW-001）の割当対象にする。
