# ADR-0152: 二組のframeでrenderとencodeを重ねる

- 状態: 部分置換（ADR-0162: 同条件legのpacked buffer/video encoder共有）
- 日付: 2026-10-11
- 対象: PERF-004
- 部分置換: ADR-0079の単一threadの逐次frame処理。音声spool/block、tile/halo/512 MiB予算、frame/sample格子、色変換、型付き失敗、staging/publication契約は維持する。

## 背景

Movieのtile render・色変換とnative encodeは別の資源を使うが、従来は同じthreadで待った。単にqueueを加えると、producer・queue・consumer・最後のPTS/span用pending frameを合わせて四組になる可能性がある。FFmpeg handleをSend扱いする変更も避ける必要がある。

## 決定

- caller threadでGPU/注入backendのtile renderと色変換を行い、専用threadで全video legのencodeを行う。RGBA8/HDR RGBA64のper-leg bufferを二組だけ確保し、所有権を循環させる。queue容量1だけをmemory上限の証拠にしない。producer/queued/consumer/recycle/finishの全ownershipを合わせて二組であり、native codec内部buffer、tile scratch、metadata、driver資源は別に計測する。
- native threadは選択済みRuntimeと同じcanonical directory・substitution状態から自分のRuntimeをopenし、全capabilityが一致しなければ失敗する。Rc・native pointer・mutable error bufferをthreadへ渡さず、unsafe Sendを追加しない。全encoderの初期化完了を受けてから最初のframeをrenderする。
- 既に固定したframe gridから各tickと次tickとの差を先に決める。最終spanは従来どおり前のspan（単一frameは1）を継承する。consumerの追加pending frameを省き、PTS/span metadataのnative照合とflushを維持する。
- GIFのhistogram/spool/固定palette/第二段階も専用threadが所有する。第二段階は既存の二組から一組を渡して読み戻し、第三のfull-frame bufferを作らない。checkpointごとにcallerへgateを送り、許可されるまで次frameをencodeしない。
- checkpoint callbackとworker job stateはcaller threadで実行する。どのrender/checkpoint/consumer失敗でもchannelを切り、専用threadをjoinしてからstageを回収する。consumerの型付き失敗をchannel切断の一般エラーへ置き換えない。全legのprobe・no-clobber・最終cancel fence後のpublicationはcallerが従来どおり行う。音声単独ではvideo threadもbundleも作らない。
- `VideoPipelineMode::Serial` はcontrolled測定用の同じ境界・一組の直列処理。既定は `TwoFrames`。resource実行policyであり、作品・request・codec・snapshot意味版やGUI操作へ保存しない。Serial比較は従来の単一thread binaryの比較とは区別する。

## 検証

per-leg二つのallocationがbackpressureとGIF第二段階を越えて循環すること、callback/backendのcaller thread、失敗時join、GIF gate cancel、typed consumer errorを通常テストで確認する。実FFmpeg/Metalのdelivery・HDR・streaming回帰、同一binaryの直列/二組でencoded bytes・独立decode・PTS/span・sample range・checkpoint、cold/warm p50/p95とprocess RSSを [PERF-004](../testing/perf-004.md) に記録する。FPSや静穏hostの性能合否はこれだけで決めない。
