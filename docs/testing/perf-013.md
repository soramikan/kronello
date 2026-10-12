# PERF-013: 長尺metadata・receipt・mux確定工程

状態: `in_progress`。PERF-011/012の必須検証・受け入れを完了し、実装へ進む。[ADR-0165](../adr/0165-bounded-export-metadata-and-receipts.md)は提案であり、chunk writer/reader/query/recoveryの実装済みを意味しない。

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

## metadata基盤の実装中

render層へ共通header・最大64frames/4MiBの差分chunk・逐次indexのwriter/readerを実装した。headerは8MiB、pageは64frames/16MiBの上限で、manifest identityはrender hash・有理数frame rate・絶対開始frame・countに結び付ける。writerは順序/time/identityと共通部分を確認し、未完了directoryはindexを閉じてから回収する。

全体検証は全chunkのhash/長さ/count/連続rangeと元の`FrameMetadata`を検証する。ページ取得はroot/header/index全体を認証し、必要chunkだけを復元・hash検証する。未参照chunkのpayload破損検査を全ページで繰り返さず、publication/recoveryの全体検証と区別する。file名はordinalから実装が生成し、indexへ任意pathを保存しない。

単体3件で1,000framesの全復元、63件ページのchunk境界、可変transfer情報、最大保持件数/serialized bytes、未完了回収、欠落/破損/完全再hashした順序違いの拒否を確認した。旧receipt読み込みやworker publicationを確認した意味ではない。

movieの全時刻・PTS Vecは定数サイズの有理数`FrameSamplePlan`とordinal PTSへ置換した。既存のnegative/NTSC/empty/overflow grid試験とmedia streaming比較（3 passed・4 ignored）が成功し、render/media全target Clippyも成功した。frame metadataのVec自体はexport/receiptへの接続が残るため、まだ全体の有界化や速度改善を主張しない。

残件はinline上限/manifestのexport接続、publication root/indexと小さいreceipt、summary/page Query、旧receipt復旧、probe再利用・mux取消、長尺のbefore/after実worker比較と全必須検証である。PERF-014も未着手である。
