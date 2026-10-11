# ADR-0156: 有界圧縮demuxと永続hardware cursor

- 状態: 採用（PERF-005の最終縦断受け入れは別管理）
- 日付: 2026-10-11
- 部分置換: [ADR-0081](0081-guaranteed-metal-hardware-video-decode.md)の毎要求の全圧縮clip保持・session作成と、そのclip全体の128MiB/262144sample上限。対応色/codec、hardware required/query、same-device import、HAL所有権、型付き失敗、softwareへの暗黙fallback禁止は維持する。

## 決定

`ResidentFileDecoder` はlocal canonical file、stream、SDR出力形式、locked半開区間を固定した所有threadの派生cursorとする。純粋モデルへnative型を追加しない。まずAVFoundationの未展開圧縮sampleをdecode orderで一度走査し、全sampleの既存色/codec/chroma/depth契約、最小output PTSと実際の並べ替え幅を検査する。一度に保持する圧縮sampleは一つ、single sample128MiB以下かつ1sample/frameとする。ゼロsample edit markerは従来どおり除外する。clip全体のarrayや圧縮spoolを持たない。AVFoundationの内部buffer/metadataはこのpayload制限とは別である。

走査で、各位置までのPTS最大値から後続PTSを引いた正の最大差 `G` を正確な有理数として求める。prefixの最大PTSを `M` とすると、以後のすべての圧縮PTSは `M-G` 以上である。全clipの検査で証明したこのwatermarkが要求時刻より大きくなるまで元のdecode orderを提出し、各提出をVT waitで完了させる。完了callbackから最大の `PTS <= time` を選ぶ。FPS、GOP長、key flag、一定のreorder frame数は推測しない。最小output PTSをlocked originへ正確に移す既存のMOV edit-list規約を維持し、負originを0へ丸めない。native整数ratioのoverflow/不正時刻は拒否する。

一つのhardware-required VT sessionを順方向で継続し、未使用の未来outputも保存する。codec変更をsessionが受け入れない場合は型付き失敗とする。callbackとcursorの出力queueはそれぞれ最大64件/512MiBで、未知のnative buffer sizeも拒否する。queueへの保持は入場前にbyte/countを検査する。callback一時所有、呼び出し元へ返したframe、native codec reference surfaces、AVFoundation内部、GPU importはqueue予算とは別で、process RSS上限と呼ばない。返すpixel bufferは独立retainする。VT wait/invalidate時もcallback Boxは生き、HAL drop tokenの既存所有権を変更しない。session/cursorに新しいunsafe Send/Syncを与えない。

直前の選択PTSより前の要求は同じcanonical originからreaderとVT sessionを新しく作り、正確に再復号する。hardware random seekはまだ高速化しない。範囲外、source変更、native/callback/budgetエラーはcurrent/future/session/readerを捨てる。破損した圧縮packetのdecodeエラーは実際に要求の証明prefixを復号するときに報告する。未提出の未来packetまで毎要求復号する従来の副作用を維持しない。全clipのformat admissionはmetadata走査で残す。

Unix file fingerprint（device/inode/length/mtime/ctime）を走査の前後と要求の前後に検査する。変更したcursorは再open/revalidationが必要で、そのまま成功させない。backendは既存asset hashの検証に成功した後でも古いfingerprintのcursorを退避し、新cursorを入場させる。同内容の書き換えでもnative readerを再利用せず、正しい固定assetの要求をstale cursorの失敗へ変えない。strict `ResidentVideoRenderBackend` はcanonical path、全asset lock/content hash、stream、SceneKey(node/instance)をキーに最大8cursorをLRU保持し、保持outputの合計512MiBを超えた分を退避する。既存の前後全量asset hash検査、native dimensions/color lockは維持する。serviceのsequence/movie処理範囲を越えてnative cursorを保存しない。入力全体のsemantic namespaceをv2へ更新する。

公開の一発 `decode_file`/`decode_file_with_interval` は独立origin oracleとして従来の全clip上限を維持する。ProRes RAWの明示経路は本変更のSDR cursorへ混ぜず、既存の保証範囲を維持する。supported H264/HEVCタグとmissing/HDR/10bit/full-range rejectionも変えない。

## 検証oracle

`read_for_validation` の展開RGBだけを、既存resident SDR shaderと同じYCbCr→encoded RGB `[0,1]` 境界に合わせる。PERF-005で見つかったCPU oracleのclamp欠落を修正するもので、production shaderやnative planesを変更しない。全native planeのbyte一致と正確なPTSを先に比較し、その後のworking-space GPU比較は既存閾値を維持する。検証readbackはproduction転送countersに含めない。

## 検証

CFR/VFR/B-frame、H264/HEVC、BGRA8/NV12、負のlocked origin、forward/backward/repeated区間、half-open end、source ctime変更、cursor破棄後の返却frame所有権を実機で検査する。[PERF-005](../testing/perf-005.md)に前後/同じbinary対照のp50/p95、全native output照合、36分clipの有界demux診断とRSSを記録する。cold metadata走査とorigin再復号、queue外のnative memory増加を隠さず、FPSやprocess memoryの保証にしない。
