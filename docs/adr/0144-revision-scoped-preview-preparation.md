# ADR-0144: revision単位の検証済みプレビュー準備

- 状態: 部分置換（[ADR-0147](0147-indexed-timeline-and-proxy-preparation.md) がPreferの準備cache除外を更新。その他の条項を維持）
- 日付: 2026-10-11
- 対象: PERF-003（プレビュー準備の範囲）
- 部分置換: ADR-0083 / ADR-0056 の通常モードにおける要求ごとのstore開閉。安全モード排他・FIFO・明示project path・共有Command/Queryを維持する。

## 背景

同じCompositionを100/1000/5000クリップへ配置するrelease測定で、native sessionの毎フレームの文書読込・snapshot構築・検証に規模依存の費用を確認した。通知頻度だけの分離では定期確認に当たる要求の待ちも残る。

## 決定

- Unixの通常 `ProjectSession` はstore接続を保持する。要求ごとにcanonical pathのdevice/inodeを照合し、置換には `PROJECT_REPLACED` を返し再openを必要とする。Windows等の通常モードは同等の置換検出が整うまでtransient openを維持する。safe modeの従来の排他は維持する。
- `ProjectStore::revision()` はdocumentを読み出さず、現在のcommitted revisionを取得する。準備時に読み取ったsnapshot自身のrevisionを保持する。外部commitと競合した場合も、古い内容と新しいrevisionを組み合わせない。
- sessionに共有serviceの派生準備を1件保持する。keyはrevision・canonical path・target・profile・font/LUT locators/locks等。regionと時刻は含めず、出力サイズ変更と時刻変更は同じ不変入力から評価する。
- font/LUTのbytes・解析済みLUTは、size・mtime・creation timeとUnix device/inode/ctimeのfingerprintが一致する間だけ再利用する。miss時は従来のhash・font face identity・LUT parse検証を実行する。読込前後のfingerprintが変われば保持しない。欠落や同じsize/mtimeでの内容変更は型付き診断を維持する。
- `ValidatedRenderSnapshot` は全体検証に成功したowned snapshotとstatic sequence loweringを保持する。mutable accessとDeserializeを提供しない。通常のraw snapshot入口は引き続き検証する。
- prevalidated経路でも時刻に依存する評価・逆方向Compositionの有理数grid sampling・Scene IR・Render DAGは毎回構築する。static sequence loweringのcloneへ時刻依存のreverse samplingだけを適用する。
- bounded `RenderCache` をsessionの寿命に結びつける。既存の意味key・layout/geometry等の容量制約を使う。DAG全体をrevisionだけで固定しない。
- 同じprojectと同じ参照先に対するtemplate reachable検証は、1回のcompile/validation内で重複を除く。配置ごとの入力値とinstance identityの評価は維持する。
- 準備cacheはsnapshot/static rootのserialized payloadとfont bytes合計256MiB以下だけを保持する。古い入力は新しい準備前に解放する。この値はRSSやallocator込みの実メモリー上限ではない。semantic cacheの既存budgetは別に持つ。
- `project.info`の派生結果/hashはrevision単位で再利用する。作品状態の正本はSQLiteのままであり、GUI専用のrevision・documentを作らない。
- `media_proxies=prefer` は資産ファイルのready/stale状態にも依存するため、この準備cacheから外し既存のprune/verification経路を維持する。proxy keyとGUI接続はPERF-003の残件。

## 検証

100/1000/5000クリップのcold/warm、stateless control/sessionのrelease測定とOS peak RSSを比較する。DAG identity、時刻・region変更、外部revision、font/LUT変更/欠落、file replacement、安全モード排他を確認する。記録は [PERF-003](../testing/perf-003.md)。decoder・GUI索引化を含むPERF-003全体の完了とは区別する。
