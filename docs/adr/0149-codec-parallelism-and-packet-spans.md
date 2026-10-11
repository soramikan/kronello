# ADR-0149: Codec別並列度と遅延packetのspan

- 状態: 採用
- 日付: 2026-10-11
- 対象: PERF-004 のnative video encoder

## 背景

ProRes KSとDNxはthread_count=1、SVT-AV1はlp=1だった。native shimの1/2/4/8比較でProResとDNxに改善候補があり、SVTの並列化では同じ入力の反復出力も変化した。また、packet durationを最後にsubmitしたframeのspanから設定しており、遅延packetと非等間隔PTSを組み合わせると別frameのspanを渡していた。

## 決定

- `prores_ks` はthread_count=4、DNxは2を初期設定にする。測定した640×360のnative出力は1threadとencoded file/decoded画素が一致した。全解像度での倍率を保証せず、製品経路の追加検証を行う。
- SVT-AV1はthread_count=1、lp=1を維持する。native比較の2/4/8で反復file hashとdecoded画素が一致しなかったため、速い1試行だけで採用しない。hardware encoder、その他のvideo codecとGIFも既存設定を維持する。
- bitrate、requested preset、codec/profile/pixel class、SDR/HDR変換とsoftware/hardware選択を変えない。Runtimeがrequested presetを補正した場合、実際のnative logの値を測定条件として区別する。
- submitしたPTS/spanを最大256件のmetadataで保持し、receiveしたpacketのcodec time base上のPTSで対応するspanを取得してからmux time baseへrescaleする。最後のsubmitのspanを流用しない。image frameのRust/C所有境界は維持し、codec内部のframe-thread bufferはpayload保持と区別する。
- 未照合PTS、metadata保持上限、flush後に残る未出力frameは型付き `ENCODE_ERROR` にする。packetの欠落を成功扱いしない。エラー時のstage回収・no-clobber publication・最終checkpointを維持する。
- 公開 `EncodeFrame` の既存動作（次PTSまでのspan、最後は直前のspanを継承、単一frameは1tick）を維持する。通常movieの等間隔frame格子にも同じ照合を使う。

## 検証

[PERF-004](../testing/perf-004.md) にnative実測と独立decoded oracleを記録する。非等間隔tick `0,1,3,6,10` とspan `1,2,3,4,4` を使い、mux直前のpacket照合とflushを確認する。通常テストではProRes/AV1のdecoded `[pts,end)`・区間中点・stream duration、およびSDR/HDR・複数出力・各音声codecの回帰を確認する。
