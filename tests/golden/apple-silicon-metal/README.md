# Apple Silicon + Metal の共通基準画像

[ADR-0047](../../../docs/adr/0047-apple-silicon-metal-golden.md) に従い、Apple Silicon ネイティブ + Metal で一つの基準を共有する。機種・OS・driver は provenance のみ。21 シーンの初回基準は QA-003 で clean commit から M1 開発機上で生成・明示採用した。

`scenes.json` は定義のカタログ。実測基準は `manifest.json` / `environment.json` / `provenance.json` / `adoption.json` と各シーンの RGBA16F / PNG。placeholder 画像は保存しない。UPDATE 候補は自動採用しない。

- [比較・採用手順](../../../docs/testing/golden-comparison.md)
- [fixture とサイズ上限](../../../docs/testing/fixtures.md)
