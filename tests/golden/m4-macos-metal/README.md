# M4 Mac mini / Metal の基準画像

**未測定・未作成**。QA-001 では解析的 scene 定義と CPU 比較 API を用意した。
GPU-001 は harness を実装したが、この機体は M1 開発機であり基準は登録していない。
参照機の fingerprint、manifest、RGBA16F/PNG は実際の M4 Mac mini 32GB 上で採取・レビューする。
M1 の UPDATE 生成物は `target/golden/run.*/candidate/` に限り、ここへコピーしない。
このディレクトリに placeholder 画像や別 GPU の結果を基準として保存しない。

- [scene 定義](../scenes.json)
- [fixture と解析的比較](../../../docs/testing/fixtures.md)
- [固定環境の GPU 比較手順](../../../docs/testing/golden-comparison.md)

- [M0 GPU スパイク報告](../../../docs/testing/gpu-spike-m0.md)
