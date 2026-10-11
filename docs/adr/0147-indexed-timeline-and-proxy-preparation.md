# ADR-0147: タイムラインの派生索引とproxy準備再利用

- 状態: 採用
- 日付: 2026-10-11
- 対象: PERF-003 のmacOS表示と共有preview準備
- 部分置換: [ADR-0144](0144-revision-scoped-preview-preparation.md) の `media_proxies=prefer` を準備cacheから除外する条項。revision fence、入力identity、検証済みsnapshot、保持予算を維持する。

## 背景

タイムラインのclip名・欠落状態の参照ごとに素材一覧を再作成し、ID/trackの検索も全clip配列を再変換していた。16素材・8trackの生成Queryで5000clipの表示データを一巡するとreleaseで約949msを要した。これはSwiftUI layoutや動画描画の時間ではなく、MainActorでの表示データ参照の時間である。

GUIにはproxy生成・状態表示があるが、native previewへ `media_proxies` を送る選択がなかった。単にPreferを選ぶだけでは、proxyの欠落・破損を除外したsnapshotがファイル復帰後も保持されるおそれがある。

## 決定

### Queryから作る表示索引

`TimelineIndex` は共有 `sequence.query` の結果からclip ID / track / track state / asset statusの索引を作り、Query結果の代入時に置換する。clipのquery順と各track内の順序を保持する。文書変更とQuery変更で `TimelineAssetIndex` を破棄し、素材名・source参照・欠落状態を遅延再構築する。SourceRefの既知のfield組だけをlookup keyにし、未知fieldを同じ参照とみなさない。

選択・ロック・ドラッグ候補・編集操作は既存の共有APIとrevisionに従う。索引は読み取り専用の派生表示データで、別の編集可能な作品状態ではない。IDを配列位置や表示名から作らない。表示用durationのframe変換は既存の有理数・overflow規約を保ち、rate単位の小さいcacheをQuery変更で破棄する。

### Previewのproxy選択

Motion/Program viewerの素材選択を「原本」/「Proxy優先」とし、UI stateにoptionalな `mediaProxies` を保存する。旧UI stateは原本へ戻す。native requestと再描画keyの両方へ正規化した `off` / `prefer` を渡し、source monitorにも同じ選択を適用する。狭いviewerではtransportを2段にする。

これはpreviewの選択であり、書き出しは原本を使う既存契約を維持する。proxy欠落/破損/リンクstale時はADR-0119の規約で原本へ戻す。

### Preferの準備cache

文書内でproxy linkが参照するAsset descriptorを、prune前の文書から保持する。各locatorを既存のrelative-before-absolute規約で解決し、canonical pathとsize/mtime/ctime/inode等のfingerprintを準備cache keyの追加依存にする。欠落状態も保持するため、ファイルの復帰でmissになる。symlinkの解決先の変更もpath/fingerprintで区別する。

miss時は従来どおりproxyの全量hash検証と一時snapshot内のpruneを行う。準備前後でfingerprintが一致した時だけ保持する。hitではfingerprintが一致する完了済み検証を再利用する。資産のdecode時の検証も維持する。このメモ化の制約はADR-0140と同じで、metadataを完全に偽装したローカル書換えを追加で防ぐものではない。proxy descriptor payloadも256MiBの準備保持判定へ数える。

## 検証

- 表示索引の外部文書更新・素材名変更・欠落状態変更・track lock変更・rate/尺変更・clip削除と、旧UI stateのdecodeを通常Swiftテストで確認する。
- cacheのready→missing→reappeared→同size/mtimeの破損→復帰→Offを通常Rustテストで確認し、毎回stateless DAGと一致させる。
- releaseの100/1000/5000clip表示データ参照は、query更新/索引構築を別に測り、全clipのlabel/missing/track/selection/durationのchecksumを変更前と比較する。
- 同じ4K HEVC作品をOff/Preferで測定し、元のextent/時刻を維持したsource dimensions・prepare/decode時間・保持bytesを記録する。proxy画素は意図的に品質が異なり、原本とのbit一致とはしない。

proxyの時刻・cropの実装検証では、原本のstart PTSから選択streamのstart PTSへの有理数平行移動と、実寸比によるdecode cropの各軸変換を確認する。原本のlayout extentとauthored documentは維持する。これはADR-0119の置換を正しい入力へ適用する実装修正である。
