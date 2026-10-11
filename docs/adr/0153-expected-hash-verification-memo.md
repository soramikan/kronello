# ADR-0153: 資産検証メモを期待hashへ結び付ける

- 状態: 採用
- 日付: 2026-10-11
- 対象: PERF-004の動画source検証監査
- 部分置換: [ADR-0140](0140-media-session-resident-preview.md) の検証メモの値と対応OS。decoder・色・保持予算は維持する。

## 背景

canonical pathとFileFingerprintだけのメモは、同じファイルに異なる期待content hashを指定しても以前の成功を再利用した。実際の動画decodeとSequentialVideoRenderBackendで、ファイルを変更せず期待hashだけを64個のzeroへ変えた場合に画素を返すことを再現した。metadata不変のローカル書換えとは異なり、文書の期待値を確認しない実装上の欠陥である。

## 決定

メモはcanonical pathごとに一件とし、値に検証前FileFingerprintと期待content hashを保持する。両方が一致した時だけ完了済み検証を再利用する。AssetIdは内容検証の条件へ加えず、同じ内容を参照する別IDは再利用できる。期待hash変更時は既存の全量SHA-256検証を通し、不一致はASSET_HASH_MISMATCHとする。失敗を新しい成功として記録しない。

warm再利用はctime/inodeを取得するUnixに限定する。現在のWindows/fallback fingerprintではin-place書換えとmtime復元を十分に識別しないため、各アクセスの全量hash検証を維持する。Windowsの強い変更identityを実装・検証してから再利用を拡張する。Unixの残余制約、検証前statを記録する順序、runtime内の寿命、relative-before-absoluteと一発resolveの厳密検証はADR-0140のまま。

## 検証

実ファイルを変更せず期待hashだけを変える回帰テストは修正前に失敗した。修正後はdecoder poolに到達する前に拒否し、その後の正しい期待hashによる描画を維持する。既存の別AssetId、forward/reverse、実ファイルのhash変更、音声sourceの再検証も照合する。Windowsは実行未検証で、性能値を主張しない。実行版ごとの結果は[PERF-004](../testing/perf-004.md)で区別する。
