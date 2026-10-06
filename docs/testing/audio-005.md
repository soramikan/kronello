# AUDIO-005 圧縮音声 AAC・Opus 採用profileの受け入れ記録

状態: `done`（2026-10-07）。`codex/m5-completion` の作業ツリーで受け入れた。mainへのマージ・各OS CIの保証とは区別する。

[提案書](../roadmap/m5-compressed-audio-proposal.md)の AAC-LC と Opus の双方を所有者が採択し、[ADR-0106](../adr/0106-versioned-compressed-delivery-audio.md)で決定した。採用profileは4件: `H264AacV1`・`HevcAacV1`（MOV）、`Av1Mp4AacV1`（MP4）、`Av1WebmOpusV1`（WebM）。`DeliveryAudioCodec` に `Aac` / `Opus` を追加し、MP4へのOpus収録は型付き拒否とする（FFmpegがMP4内Opusのdiscard情報を復号時に反映せず、実測で終端sampleが一致しなかったため）。

## 実装範囲

- `crates/kronello-media/native/media.c`: audio encoder を kind 別テーブル（ALAC / AAC-LC / Opus）へ一般化し、priming / discard の取り扱いを encoder ごとに分離した。Opusは終端trimを表現できない1 packet 未満（content ≤ 648 sample = block − pre-skip）の入力を型付き拒否する。
- `crates/kronello-media/src/{audio,export,ffi}.rs`: `MovieProfile` に4 variant を追加し、lossy音声では stream duration が無い container（WebM）の format duration fallback と、mux後の duration/pts を bounded error で検証する。
- `crates/kronello-service/src/export_profiles.rs`: `DeliveryAudioCodec::{Aac,Opus}`、profile 一覧、wire の `av1_webm` 形式を接続した。
- `scripts/native-dependencies.json`・`scripts/build_ffmpeg_lgpl.py`: opus 1.5.2（BSD-3-Clause、SHA-256 `65c1d2f78b…` でpin）を LGPL 配布構成の source manifest と build に追加した。配布物は共有ライブラリのみで、GPL/nonfree 構成は検証で失敗する。

## 受け入れ証拠

`python3 scripts/audio_005_evidence.py` を実行し、`target/m5-acceptance/audio-005/evidence.json` に status `passed` を記録した（platform: macOS 27.0.1 arm64、記録revisionは未コミット作業ツリーのHEAD `adc84d3`）。5コマンドすべて exit 0。

| 検証 | 内容 |
|---|---|
| `cargo test -p kronello-media --test audio5 --locked` | 5/5。`aac_and_opus_delivery_audio_exact_length_bounded_error` で両codecの duration/PTS が厳密長・bounded error 内。`h264_aac`/`hevc_aac`/`av1_mp4_aac`/`av1_webm_opus` の各 movie profile を実 encode→decode で検証 |
| `cargo test -p kronello-service --test media --locked` | media service の回帰全体 |
| `cargo test -p kronello-service export_profiles` | profile listing・codec列挙・未採用形式の拒否が wire 経路で一致 |
| `cargo test -p kronello-media --test audio --locked` | 音声 decode / resample の回帰 |
| `cargo test -p kronello-media --test profiles --locked` | 既存 ALAC / PCM24 profile の回帰 |

補足の測定として、AAC-LC は MP4 / MOV 双方で 48000・33600 sample（frame非整列）の roundtrip が一致した。Opus は WebM でのみ厳密長を確認し、MP4収録の拒否と packet 不足の typed refusal も試験で固定した。復号側の時刻連続性は、Opusのpre-skipが負のptsを持つため、入力domain累積ではなく delivery domain（delivered sample）で検査する。

## 残る境界

- 実測はこの macOS arm64 環境の FFmpeg / libopus 構成に対するもの。Windows / Linux の encoder 版差異・hardware経路は別タスク（GPU-004/005 等）で扱う。
- profile version は `V1` 系で固定した。将来のprofile変更は版を上げて互換を保つ。
