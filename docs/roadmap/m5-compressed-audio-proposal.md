# OQ-21 圧縮音声の採用案（未採用）

2026-10-06。AUDIO-005着手前の判断資料。採用判断と、実装後の品質・同期・配布検証を区別する。

## 提案する範囲

- AAC-LC: FFmpeg内蔵`aac`を使い、48 kHz stereo / 192 kbit/sを版付きprofileに固定する。MOV / MP4を対象とし、既存H.264・HEVC・AV1出力との組合せを検証する。
- Opus: `libopus`を使い、48 kHz stereo / 128 kbit/s、20 ms frame、audio用途、VBRを版付きprofileに固定する。AV1 + OpusのMP4 / WebMを対象とする。
- PCM24 / ALACは既存profileとして維持する。任意codec・container・encoder optionの透過指定は許可しない。

公開profileは固定snapshotのjob入力に保存し、CLI / MCP / GUIの共有APIで同じ意味を使う。採用判断後に実装し、未検証の組合せを成功扱いにしない。

## 受け入れ

同梱LGPL構成へ必要なencoder/muxerとlibopusのみを追加し、source URL・version・hash・license・configureをmanifestへ固定する。`--enable-gpl`と`--enable-nonfree`は使わない。

無音・正弦波・複数周波数・短いpulse・frame境界に一致しない長さを使い、decode後の先頭時刻・終端sample数・A/V同期を確認する。AAC priming/padding、Opus pre-skip/codec delay/discard paddingを実containerのmetadataとdecode結果で照合する。品質は固定生成信号の誤差と聴取用fixtureを記録し、lossy出力を全画素・全sample一致とは扱わない。必要なら実測後にprofile設定を再提案する。

## 確認した一次資料と残る判断

[FFmpeg codec documentation](https://ffmpeg.org/ffmpeg-codecs.html)に内蔵AACとlibopus wrapperが記載されている。[FFmpegのライセンス説明](https://ffmpeg.org/legal.html)は構成依存のライセンスと、特許が著作権ライセンスとは別であることを説明している。[Opus公式ライセンス](https://opus-codec.org/license/)にはソースのBSDライセンスと特許許諾条件がある。

この調査は個別の販売地域・配布形態に対する法的判断を完了するものではない。ここで依頼するのは上記技術profileの採用と実装・実測への着手判断である。製品配布条件の確認はRELEASE-002〜004へ明示して引き継ぎ、技術検証完了を配布許諾の取得とは記録しない。
