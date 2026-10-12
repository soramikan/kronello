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

## 件数を変えた基準

[1,000frame基準](perf-013-baseline-1000.json)は同じ固定binary・document・100fps・2×2 outputで、rangeだけを`[0, 10)`へ変えた別workerである。両documentのclip countは0で、unused source assetは同じidentityのまま保持する。source decodeの費用を長尺metadataの増加と混同しない。

| 条件 | 1,000frames | 10,000frames |
|---|---:|---:|
| submit→terminal result | 1,529.4ms | 10,848.5ms |
| render/色変換 | 65.0ms | 615.4ms |
| receipt/hash保存 | 1,290.5ms | 9,485.1ms |
| inline frame JSON | 2,414,729bytes | 24,177,209bytes |
| receipt | 2,422,012bytes | 24,184,560bytes |
| sampled worker RSS peak | 75,284,480bytes | 565,215,232bytes |

各条件1試行で、workspace検証と並行した静穏でないhostの観測である。RSSをmetadataだけへ帰属させたり、一般的な改善率を算出する証拠にはしない。frame数に比例するserialized bytesと、それを全量保持/JSON化する現在の実装を変更後の保持上限・ページ復元と比較する。
