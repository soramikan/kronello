# ADR-0068: 版付き配信映像と ALAC 音声の出力契約

- 状態: 採用（AAC / Opus の採用は保留）
- 日付: 2026-10-05
- 対象: MEDIA-002
- 追加範囲: ADR-0035 / 0036 / 0048 / 0049 / 0050 / 0063 / 0069 の閉じた追加出力。既存 ProRes / PCM24 と movie profile 1/2/3 の意味・既定値は維持する。

## 決定

共有 `render.export` / `render.submit` の `JobOutput` に次の閉集合を追加する。
任意 encoder 名・FFmpeg 引数・shell・URL を公開しない。`profile_version` は追加形式では必須。
未知版・未採用 audio codec は `UNSUPPORTED_FEATURE`、拡張子の不一致は `INVALID_MEDIA_INPUT`。

| format / version | container / 拡張子 | video encoder | audio |
|---|---|---|---|
| `av1_mp4` / 1 | ISO BMFF MP4 / `.mp4` | `libsvtav1` 固定、software | native `alac`、48 kHz stereo、PCM24 量子化後の lossless |
| `h264_mov` / 1 | MOV / `.mov` | `h264_videotoolbox`、hardware 必須 | 同上 |
| `hevc_mov` / 1 | MOV / `.mov` | `hevc_videotoolbox`、hardware 必須 | 同上 |

`hevc_mov` version 1 の sample entry は `hvc1` に固定する。VideoToolbox は既存の
`AV_CODEC_FLAG_GLOBAL_HEADER` を使い、コピーした extradata / parameter sets を MOV の
`hvcC` sample description に格納する。最終 mux は header を書く前に output stream の
codec_tag を `MKTAG('h','v','c','1')` に設定し、空 extradata と出力 tag 不一致を拒否する。
407fa0d の Apple M1 host では既定 `hev1` の MOV と HEVC track 単独が
AVFoundation isPlayable=false だったため修正する。未リリースの version 1 の契約確定であり、
profile version は増やさない（supervisor 指示）。hardware / fallback / audio の意味は変更しない。

FFmpeg 9.0.2 の実 CPU 検証で MOV mux header が `av1 only supported in MP4 and AVIF.` と拒否した。
この結果に基づき supervisor 承認で AV1 を MP4 にした。MOV 名のまま MP4 を出力しない。
H.264 / HEVC は `allow_sw=0` を維持する。登録がない、hardware-capable でない、device open が失敗する場合は
`ENCODER_UNAVAILABLE`。x264 / x265・OS software encoder・他形式へ自動代替しない。
新 AV1 profile は旧 low-level AV1 選択の libaom 候補へ戻らず、SVT 不在なら `ENCODER_UNAVAILABLE`。
報告に実 encoder、software / hardware、RGBA→YUV conversion と CPU payload upload 経路を保持する。
GPU 常駐・driver 内部転送量・hardware decode を保証しない。
追加profileの映像は既存SDR境界のBT.709 opaque RGBA8→YUV420P（8 bit / limited range）。
bitrateは2,000,000 bps、codec thread_countは1、SVTはpreset12 / lp=1（固定4.2.0では実preset11）。
品質presetの任意指定は公開しない。全寸法の受理・全player互換性を保証するprofileではなく、
encoderが拒否した寸法/設定はENCODE_ERROR、hardware open failureはENCODER_UNAVAILABLEとして失敗する。

追加形式の `audio` は `explicit | document | silence`（省略 explicit）、`clips` と `background` は明示する。
`audio_codec` は閉集合 `alac | aac`（省略 alac）。AAC は要求を保持できても必ず型付き未対応で失敗する。
document / silence と非空 clips の併用は従来どおり `INVALID_MEDIA_INPUT`。
出力 profile の version 1 と audio evaluator の version を混同しない。追加出力は常に
AvExportSnapshot schema 3 / evaluator 2（movie profile 3 の音声意味）を使う。
`movie_profile` を envelope 全体 hash に含め、RenderSnapshot hash と区別する。
旧 envelope / report は新 optional field を省略して byte/hash の契約を維持する。

## ALAC と終端・同期

入力 Bus は既存の 48 kHz stereo f32。PCM24 の nearest / ties away from zero、dither なし、
Reject / 明示 Saturate と clipping count を再利用し、上位24 bitの S32 を planar S32P へ並べ替える。
native ALAC の `bits_per_raw_sample=24`、`initial_padding=0` と SMALL_LAST_FRAME 対応を要求する。
encoder の frame_size（固定 runtime は4096）を使い、最後は残った実 sample 数だけ渡す。
無音 padding・priming・sample discard を追加しない。flush 後も全 packet を drain する。
ALAC 自体は量子化後の lossless であり、元 f32 を無量子化で保存する契約ではない。

音声 stage は明示 MP4。MOV の単独1 sample ALAC stage は FFmpeg 9.0.2 の demux が
`Zero bytes per frame, but 4096 samples per frame` として decode できなかったため、
試験済みの MP4 stage を使って最終 container へ packet copy する。これは固定した内部経路であり
失敗後に別形式へ retry する fallback ではない。PCM24 stage は従来 MOV のまま。
新音声 stage / 最終 mux の movie timescale は48000。映像 track timescale は正確な frame tick の分母。
PTS / DTS / duration は rational のまま rescale / interleave し、B-frame の DTS と presentation PTS を分ける。

映像評価は元の絶対 frame 時刻、音声は同じ絶対 range の `floor(time*48000)` 格子。
ファイル先頭は両 stream とも0。N samples の最後の sample PTS は `(N-1)/48000`、
exclusive end は `N/48000`、映像 end との差は厳密に1/48000秒未満。
probe は閉 profile の codec、2 stream、48 kHz / 2 channel、zero-origin、duration と両 snapshot metadata を検証する。
入力 stage と出力の start / duration が厳密に一致した後だけ no-clobber publication する。
job は従来の lease / cancellation fence と atomic NOREPLACE rename を使う。
検証では量子化した evaluator 出力の全 channel sample と decode 後を bit 単位で比較し、
先頭・末尾 video PTS と audio sample count / start / end を照合する。最終 decoder / player の受理は別の host gate。

## AAC-LC の将来契約（未採用）

AAC-LC を採用するなら ISO BMFF MP4 / MOV、48 kHz stereo、明示固定 bitrate / quality と
native `aac` encoder の版を固定し、量子化境界も別途定める必要がある。
LC の通常1024 sample coding frame と実際の `AVCodecContext::initial_padding` を読み、
encoder delay / priming と入力 sample を区別する。1024という例を固定値として推測・二重加算しない。
先頭の負 PTS packet、skip-samples side data、MOV edit list と iTunSMPB が示す priming / end padding の
優先順位・整合性を定め、重複 trimming や非整合 metadata の無視を禁止する。
最後の coding frame の padding を入力N samplesと区別し、decode 後は厳密にN samplesを返すことを要件とする。

採用前に impulse / tone / silence、1024の倍数と非倍数、先頭・最後の impulse、NTSC / 非ゼロ絶対 rangeで
codec packet PTS、edit list、decoded first / last sample の対応と A/V sync を確認する。
lossy roundtrip は ALAC の bit 比較を流用せず、品質・振幅誤差・時間位置の基準を明示する。
FFmpeg native AAC の品質評価と distribution / patent review は未実施であり、採用を保留する。
LGPL source license から特許許諾の結論を導かない。supervisor が未決事項を別途管理する。

## AV1 の Web 配信音声と配布

今回の AV1 / ALAC MP4 は両 codec を受理する player に限る。一般的な browser / AVFoundation の
再生互換性を保証しない。ffprobe の codec / packet / format と AVFoundation の isPlayable / track format は
revision / platform 付きで検証文書へ記録する。407fa0d / Apple M1 の AV1 MP4 は
isPlayable=false（M1 の AV1 decode 非対応という互換性制限）、H.264 MOV と ALAC-only m4a は
playable。HEVC は `hvc1` 修正後の host 受理確認を残す。
AV1 + Opus の MP4 / WebM 配信は範囲外。libopus を同梱 LGPL build へ追加し、Opus pre-skip、
codec delay、seek pre-roll、discard padding と最後の実 samples / container timestamp grid を別契約にする必要がある。
WebM の粗い time_base を今回の sample 精度契約へ黙って混ぜない。

配布入力は ADR-0036 の固定 LGPL shared runtime。system FFmpeg は開発専用で、GPL / nonfree の
テスト成功を同梱 runtime の配布検証へ読み替えない。ALAC は既存 native LGPL encoder / decoder を使い、
外部 codec、configure flag、manifest は追加しない。SVT の license / PATENTS を含む既存表示・差し替え方針を維持する。
H.264 / HEVC の提供は ADR-0035 の hardware 限定方針であり、全用途の特許問題解決を宣言するものではない。
AAC の特許・配布判断は保留。HDR は COLOR-001、長尺 streaming / 実時間再生も今回の範囲外。

native ALAC / AAC の確認元は [FFmpeg 9.0.2 ALAC encoder](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/libavcodec/alacenc.c)、
[AAC encoder](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/libavcodec/aacenc.c)。upstream code を本体へコピーせず C shim の既存動的 API で呼ぶ。

## 検証

[MEDIA-002 の検証](../testing/media-002.md) に criterion ごとの実行結果と pending host run を分けて記録する。
