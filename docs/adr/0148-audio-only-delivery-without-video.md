# ADR-0148: 音声単独出力の映像処理省略

- 状態: 採用
- 日付: 2026-10-11
- 対象: PERF-004 の音声単独delivery
- 部分置換: [ADR-0079](0079-bounded-streaming-movie-export.md) の全deliveryでのframe metadata生成。映像を含む出力のtile処理・音声streaming・上限・publicationは維持する。

## 背景

MP3/FLACだけの出力でも、各frameのtile描画を行い、捨てる画素からmetadataを作っていた。同期serviceと固定workerも映像backendを作成し、復旧検証は常に論理frame数と実際の描画metadata数の一致を要求していた。

## 決定

- 全出力legに映像がない場合、映像backendの構築・参照・実行、映像tile評価と画素変換を省く。MP3/FLACのpixel working spaceとbackground範囲は映像用の制約を適用しない。非finite backgroundはreportのserializabilityのため拒否する。
- 固定snapshot、font/LUT/assetの検証、audio source、sample格子、mix/encode/probe、no-clobber publicationは同じ経路を使う。未知仕様や破損資産を無音へ置換しない。
- `AvExportReport.frames` は実際に描画した映像のmetadataだけを返す。音声単独では空配列。映像を一つでも含むfan-outは従来の一描画passを維持する。
- `total_frames` / `completed_frames` は既存の要求frame格子による論理進捗を維持する。decode chunk、audio blockのcheckpointと最終確定前のcheckpointを残し、最後に論理総数を渡す。音声単独のcancelで偽の中間映像進捗を作らない。
- 新規workerの成功検証は音声単独ならmetadata数0、映像を含むなら論理総数を要求する。復旧は旧workerの音声単独metadata数が論理総数である成果物も許容する。receiptの成果物hash・probe・snapshot・既存metadata検証を維持し、任意のmetadata数を認めない。
- 音声単独はGPU初期化を必要としない。映像がある出力でGPU失敗をCPUへ暗黙fallbackしない。

## 検証

backendの全参照をpanicにする実出力、HDR作品の音声単独、最終checkpointでのcancelとstage回収、GPU factory失敗の同期出力と混合出力、固定workerの成功・復旧・改変拒否を通常テストで確認する。非無音の原音を用いたrelease比較では、音声のsample数・sample範囲・decoded bit hash・encoded file hashを前後で比較する。[PERF-004](../testing/perf-004.md) に実測範囲と残件を記録する。
