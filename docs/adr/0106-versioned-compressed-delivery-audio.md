# ADR-0106: 版付き圧縮配信音声（AAC-LC と Opus）

- 状態: 採用
- 日付: 2026-10-06
- 関連: AUDIO-005、OQ-21、ADR-0049、ADR-0068、ADR-0079

## 背景

配信映像の音声はこれまで ALAC（MOV / MP4）と PCM24（ProRes MOV）に限られ、
AAC-LC と Opus は codec 名を閉集合として予約しつつ採用を保留していた
（ADR-0068 の「AAC / Opus の採用は保留」、OQ-21）。
[圧縮音声提案](../roadmap/m5-compressed-audio-proposal.md)が提示され、
実機の FFmpeg 9 上で priming / padding / remux の実測を行った結果、
次の通り採用可能と判明した。

## 決定

`m5-compressed-audio-proposal.md` の profile を採用する。決定事項は次の通り。

- AAC-LC: FFmpeg 内蔵 `aac` encoder を使い、48 kHz stereo / 192 kbit/s を
  版付き profile に固定する。新しい `MovieProfile` は
  `h264_aac_v1`（MOV）、`hevc_aac_v1`（MOV, hvc1）、`av1_mp4_aac_v1`（MP4）。
  `audio_codec: "aac"` を既存 `av1_mp4` / `h264_mov` / `hevc_mov` 出力で選択できる。
- Opus: `libopus` を使い、48 kHz stereo / 128 kbit/s / 20 ms frame /
  `application=audio` / VBR を版付き profile に固定する。新しい
  `MovieProfile` は `av1_webm_opus_v1`（WebM）で、新しい出力形式
  `av1_webm`（`audio_codec: "opus"` 固定）から選択する。
- **MP4 内の Opus は採用しない。** 実測で Opus-in-MP4 の remux / decode が
  終端 sample 数を正確に戻せない（discard padding が適用されず 48000 を
  期待する 1.0 s が 48648 sample として decode される）。`av1_mp4` /
  `h264_mov` / `hevc_mov` に `audio_codec: "opus"` を指定した場合、および
  `av1_webm` に他 codec を指定した場合は `UNSUPPORTED_FEATURE` の型付き
  拒否とする。黙った代替はしない。
- Opus の入力が priming margin（codec frame − pre-skip、48 kHz / 20 ms では
  648 sample）以下の場合、単一 packet では discard padding を表現できないため
  エンコードを型付き失敗とする。実用上の配信映像は 1 video frame 以上の
  音声を常に持つため影響は限定的である。
- PCM24 と ALAC の profile は変更しない。
- 品質は全 sample 一致ではなく、固定生成信号（無音・正弦波・複数周波数・
  pulse）に対する decode 後の終端 sample 数一致・ゼロ原点・有界誤差
  （正規化相互相関 > 0.97、無音の漏洩 < 0.001、pulse 位置一致）で検証する。
- 音声 timestamp 連続性の検査は従来の stream tick 1 分精度から 2 tick に
  緩める。WebM は packet 時刻を 1 ms に量子化し、Opus pre-skip（6.5 ms）の
  丸めが 1 tick を超え得るため。最初の負 PTS は codec delay として
  content 開始を 0 に固定する。実際の frame 欠落・重なり（20 ms 単位）は
  引き続き拒否する。
- 中間音声は AAC が MP4、Opus が WebM 単一 stream とし、最終 container へ
  packet copy で remux する。mux 時の `MovieProfile` は codec / container の
  閉じた写像表で検証する。
- LGPL-only 構成に `libopus` を加え、`opus 1.5.2` の source URL・SHA-256・
  license（BSD-3-Clause）・configure flags を
  `scripts/native-dependencies.json` に固定する。`--enable-gpl` /
  `--enable-nonfree` は引き続き使わない。

## 置換

- ADR-0068 の「AAC-LC は版付き profile 契約としては未採用」・「Opus は範囲外」
  の保留条項を置換する。`audio_codec` 閉集合に `opus` を追加し、AAC / Opus を
  それぞれ上記の版付き profile で正式採用する。PCM24 / ALAC の既存契約、
  timescale・hash・固定 snapshot などの残りの条項は維持する。
- OQ-21 は本決定で解決する。

## 検証

- `kronello-media` `tests/audio5.rs`: 無音・正弦波・複数周波数・pulse・
  非 frame 整列長（33600 / 961 / 2 sample）で AAC-LC / Opus の encode →
  probe → decode が終端 sample 数・ゼロ原点・有界誤差を満たすこと、
  priming / padding が container metadata 経由で decode 時に反映されること、
  4 profile の movie roundtrip（verify_movie + remux 後の decode 正確数）、
  短すぎる Opus 入力の型付き拒否、公開ファイルの非破壊を確認する。
- service 側では `audio_codec` の組合せ制約（Opus は `av1_webm` のみ、
  `av1_mp4` / MOV では拒否）が wire / job 層で検証される。

## 関連

- [ADR-0049](0049-audio-bus-timing-and-codec.md)
- [ADR-0068](0068-versioned-delivery-movie-profiles.md)
- [ADR-0079](0079-bounded-streaming-movie-export.md)
- [docs/architecture/05-media-export.md](../architecture/05-media-export.md)
