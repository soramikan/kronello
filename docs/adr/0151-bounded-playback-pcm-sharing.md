# ADR-0151: 再生準備の有界PCM共有

- 状態: 部分置換（[ADR-0155](0155-exact-bounded-audio-checkpoints.md): compiled plan再利用と別予算のDSP checkpoint。PCM/sourceの条項は維持）
- 日付: 2026-10-11
- 対象: PERF-004
- 部分置換: ADR-0076のdecoded source保持予算へ、再生プランとは別の派生PCM cacheを追加する。current/candidateのプラン数、source合計frame上限、producer/device境界、音声clockと型付き失敗は維持する。

## 背景

再生開始、seek、作品revision更新のたびに同じ素材をdecodeすると、映像だけの編集でも音声準備を待つ。PCMはimmutableだが、DSP/WSOLAの状態は再生プランごとに独立しなければならない。

## 決定

- `kronello-media::AudioSourceCache` はcaller所有の派生resource。defaultは32entry・PCM payload 128 MiB、LRUで両上限を守る。超過する素材はdecode結果を返すが保持しない。active planの `Arc<DecodedAudio>` はeviction/close後も有効であり、retained payloadをRSSや全active ownerの合計と呼ばない。
- native `ProjectSession` はcacheを保持し、scoped ServiceのcaptureがそのArcを準備inputへ渡す。session外のServiceは自分のcacheを使う。作品のsnapshot/revisionは共有store APIで固定する。standalone `PreparedAudio::prepare` は保持なしとし、process-global作品状態やregistry operationを追加しない。
- identityはcanonical素材path、content hash、asset kind、stream index、全stream metadata、実FFmpegのversion/library構成、decoder実装hash。AssetId/表示名/revisionからPCM identityを導出しない。同じ素材の別AssetIdは共有でき、映像だけのrevision更新でも使える。planは毎回compileし、新しい音声編集を以前のplanへ混ぜない。
- cold missは既存decode APIの前後のfull hash照合を維持する。Unixのwarm hitはlocatorを毎回解決し、size/mtime/ctime/inodeが変わっていないことを二度確認する。mtimeを戻す同じ長さの上書きやatomic replaceもinvalidateする。他OSは同等のfenceを実装するまでfresh decodeとし、cache成功へ見せかけない。
- relative候補が存在する場合の優先順位、wrong hash、非file、missing、stream metadata不整合を従来どおり拒否する。cache hitにも残source frame予算を適用し、28800000 frame上限を緩めない。
- cache lock/file確認/native decodeはpreparation threadだけ。producerはimmutable PCMへのreadだけを行い、prepared planごとの `AudioPlaybackSession` を持つ。device callbackのcopy/silence/atomics契約は変えない。

## 検証

source clock/layout/sample bits、warm Arc共有、entry/byte evictionとactive owner、budget/stream拒否、mtime復元・差し替え・削除・相対候補拒否、revision更新と独立DSP、native captureを別threadでprepareする経路を通常テストで確認する。cold/warm再生準備・最初のblock・payload・process RSSと測定限界は [PERF-004](../testing/perf-004.md) に記録する。実device underrunやDSP checkpointの改善をこのsource cacheのテストだけで受け入れない。
