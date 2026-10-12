# ADR-0168: publication rootの制限を既存出力legの受け入れ範囲と整合させる

- 状態: 採用
- 日付: 2026-10-13
- 対象: PERF-013 / PR #21レビュー修正
- 部分置換: ADR-0165のpublication root群の独立した固定上限。header/chunk/index/receipt byte上限、file数上限、hash/attempt/lease/NOREPLACEの契約は維持する。

## 問題

artifact indexの9root制限は公開exportの複数leg数と整合せず、合法な複数出力が全レンダー後のpublicationだけで失敗する。また、旧attemptのreceipt回収がindex sidecarを残す。

## 決定

- 独立した9root制限を撤去する。各rootは非空であるためroot数はartifact file数以下となり、既存の全4,000,001file budgetから上限を導く。出力legへ新しい小さな上限を課さない。
- writer/readerはline32KiB、path4KiB/depth4、buffer64KiB、全file数/hash/order/extra-file検証を維持する。readerはroot数がindexのfile countを超えるdescriptorを拒否してからroot counterを確保する。
- 新attemptのstaging準備時に、旧attemptの`*.artifacts.jsonl`とreceiptを両方回収する。receiptが既に消えていてもsidecarを回収する。現在のattempt、成功結果のprune保持、無関係なfileの保護を維持する。

## 検証

12rootの公開とrename後の復旧、partial receiptと孤立indexの回収、無関係なsidecar保持、既存jobs全試験を[PERF-013](../testing/perf-013.md)に追記する。
