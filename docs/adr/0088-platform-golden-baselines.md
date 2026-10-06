# ADR-0088: GPU golden の環境別基準と software adapter の明示

- 状態: 採用
- 日付: 2026-10-06
- 部分置換: ADR-0047 の比較環境と同梱合計サイズ。Apple Silicon 共通基準と `2^-10` は維持する。

## 決定

`apple-silicon-metal`、`linux-vulkan`、`windows-dx12` の三つを個別の採用対象とする。Apple Silicon は M1 と M4 で共通基準を比較する。Linux / Windows は実際の adapter が選択された API と一致することを検査し、CPU device（Mesa lavapipe / Microsoft WARP を含む）は `software_adapter` と `device_type` を provenance に記録する。software adapter の実測を hardware GPU の証拠と呼ばない。基準と比較実行の device class が違えば拒否する。

UPDATE、明示採用、通常比較を分離する。CI は候補生成後に採用済み基準へ比較し、基準欠落を失敗とする。候補を自動採用しない。dirty 候補、revision 不一致、hash 不一致、入力不一致、非有限値、ゼロシーンを引き続き拒否する。40 シーンの全画素を CPU oracle と基準へ比較し、許容誤差・比較版は変更しない。

三基準を保存するため、同梱ファイルの上限は 1 ファイル 256 KiB、fixtures と全 platform goldens の合計 3 MiB とする。採用時に全体を集計し、上限超過を拒否する。

## 検証

実測と採用状況は [QA-004](../testing/qa-004.md) を正本とする。harness 実装だけでは Linux / Windows の基準登録や M4 校正を完了と扱わない。
