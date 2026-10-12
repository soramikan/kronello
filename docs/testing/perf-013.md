# PERF-013: 長尺metadata・receipt・mux確定工程

状態: `planned`。PERF-011/012の必須検証を完了してから実装へ進む。[ADR-0165](../adr/0165-bounded-export-metadata-and-receipts.md)は提案であり、chunk writer/reader/query/recoveryの実装済みを意味しない。

## 変更前の実worker基準

[基準記録](perf-013-baseline.json)は既存の固定10,000frame GIF fixture（100fps/100秒、2×2 output、CPU reference、disk persistence無効）を`render.submit`→`succeeded`で完走したもの。sourceやdocumentは既存fixtureを使い、binaryは`1f05c4382d8400f22b87883c50ab8104f9ab0c26ae0bcd22acb99b2213a034d4`、新しいSQLite依存へ更新する前の固定buildである。一般的なmovie/GPU性能の基準ではなく、軽いbackendでmetadata経路を露出させる。

源資料は`target/perf-013/baseline-long-metadata/evidence.json`、44,829,795bytes、SHA-256 `aca0332d63fe077b7485b0221784eb255c8db1a73900e122e865fb15792db4d9`。全10,000件のframe metadataを保存している。compact表現のcanonical count/hashと源資料のbyte hashを区別する。

- submitからterminal resultまで約10,848.5ms。
- workerの`publication_receipt_hash_write`は約9,485.1ms、render/色変換は約615.4ms。
- inline frame metadataのcompact JSONは24,177,209bytes、publication receiptは24,184,560bytes。
- workerのsampled RSS peakは565,215,232bytes。source cache/native/driver等を含み、metadataだけの保持量やprocess maximum RSSと扱わない。

この1試行から一般的な速度差を予測しない。変更後は新binaryの固定hashで交互比較し、全metadataの復元、frame range/count、全decoded bits/PTS/packet、reader/query/recoveryの保持量と破損拒否を別々に証明する。小さいheader/chunkへの移行に加え、root receipt自体のartifact一覧とserialization I/Oも検証対象にする。
