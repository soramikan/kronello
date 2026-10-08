# AUDIO-010 ピッチ保持リタイムとマルチチャンネル（5.1ch 等）の受け入れ記録

状態: `m9-lane-a` 作業ツリーで確認済み（2026-10-08）。実装は ADR-0124 の決定に従う。

## 実装範囲

### ピッチ保持リタイム（決定的 WSOLA）

- `kronello-model`: `AudioRetimePolicy::PitchPreserveV1`（wire 値 `pitch_preserve_v1`）を追加。`audio_retime` はクリップ単位の宣言であり、タイムマップは既存の有理数時間系から導出される。
- `kronello-audio/wsola.rs`: 同期窓 1024・合成ホップ 512・オーバーラップ 512・探索半幅 256 の決定的 WSOLA。正規化相互相関と固定候補順（0, -1, +1, …）・厳密 `>` タイブレーク・f64 蓄積で、乱数・外部プロセス・プラットフォーム DSP を使わない。
- speed=1 はオーバーラップ加算をバイパスして `resample_v1` と同一出力。hold 区間は明示的な無音。非対応マップ・ソース不足は `UNSUPPORTED_FEATURE` / `AUDIO_SOURCE_TOO_SHORT` の型付きエラー。
- `AdvancedAudioPlan`（document_audio 経路）が `Source::PitchPreserved` として評価に組み込む。ジェネレータ・コンポジションソースでは型付きエラー。

### チャンネルレイアウトとマルチチャンネル

- `kronello-model/channel.rs`: `ChannelMask`（版付き u64 bitset、FFmpeg/SMPTE のスピーカービットに準拠）。クローズドセットは mono / stereo / 5.1(side) / 5.1(back) / 7.1 のみ。未知マスク・チャンネル数不一致は `UNSUPPORTED_CHANNEL_LAYOUT`。
- `kronello-audio/channels.rs`: `ChannelBuffer` / `ChannelBus` / `ChannelSources` / `ChannelSourceReader`。バスはレイアウトを保持し、`into_stereo_bus` は非ステレオを拒否する。
- ダウンミックスは ITU 係数（center -3 dB、surround -3 dB、LFE 既定除外）の決定的係数表 `downmix_matrix`。出力 `channel_mask` に応じて明示変換するのみで、暗黙の fold-down はない。
- 各音声エフェクトはチャンネルごとに適用（compressor/limiter のみ LFE 除外規則）。PCM24 量子化は既存の共有規則を全チャンネルに適用。

### メディア境界とエクスポート

- `media.c` / `ffi.rs` / `audio.rs` / `streaming.rs`: デコーダは FFmpeg `AVChannelLayout` のネイティブマスクを公開し、>2ch でマスク未指定の素材は `UNSUPPORTED_CHANNEL_LAYOUT`。エンコーダ・mux はチャンネル数とマスクを受け、probe は `channel_mask` を報告する。不一致は mux で型付き拒否。
- `export.rs`: `AvExportSnapshot::with_audio_layout` がスナップショット恒等性にレイアウトを含め、音声エンベロープ 3 を固定する。`encode_audio_channels` は非ステレオバスをそのまま書き出す。`verify_movie_layout(profile, layout)` が codec・チャンネル数・マスク・PTS を出力検証する。
- `kronello-service`: `JobOutput` の全ムービーバリアントに `audio_layout: Option<ChannelMask>`（省略 = stereo の後方互換）。`movie_settings` は非 stereo に `profile_version` 3 を要求し、submit/worker 双方で `movie_snapshot` → `export_av` → `verify_movie_layout` を通す。
- `schemas/api-v1.schema.json` / `project-v1.schema.json` を再生成済み（`pitch_preserve_v1`、`ChannelMask`、`audio_layout`）。

## 受け入れ証拠

この作業ツリーで実行（2026-10-08）:

```
cargo test -p kronello-audio --test audio10 --locked   # 4 件成功
cargo test -p kronello-media --test audio10 --locked   # 5 件成功
cargo test -p kronello-cli --test jobs --locked -- surround_audio_layout multichannel_layout   # 2 件成功
cargo test -p kronello-service --test nle_schema --locked   # スキーマ再生成・一致
```

| 確認項目 | 内容 |
|---|---|
| 決定的 WSOLA | `tests/audio10.rs::wsola_retime_preserves_pitch_and_duration_deterministically`: 240 Hz 正弦波を speed 2 でリタイムし、出力 duration がプレースメントの有理数時間どおり 24,000 フレーム、自己相関の主周期がソースの 200 サンプルに保たれる（`resample_v1` では 100）。再評価と分割レンダーの継ぎ目が連続レンダーとビット一致し、piecewise slope 0 の hold 区間はホップアンカー（12,288）以降が無音 |
| 型付き拒否 | `wsola_rejects_unsupported_maps_and_short_sources_with_typed_errors`: ソース不足（48,000 消費に対し 24,000 サンプルのソース）は `AUDIO_SOURCE_TOO_SHORT`、reverse との組合せはコンパイル時に `UNSUPPORTED_FEATURE`。`ChannelMask::from_bits(0x607)` は `UNSUPPORTED_CHANNEL_LAYOUT` |
| 5.1 保持・明示ダウンミックス | `multichannel_sources_keep_layout_or_downmix_only_through_the_matrix`: 5.1 ソースが bus にそのまま保持され、stereo 出力は係数表どおりの明示ダウンミックスに一致 |
| 量子化 | `channel_bus_quantizes_every_channel_with_shared_pcm24_rules`: 6ch の +0.5/-0.5 が共有 PCM24 規則で量子化される |
| ネイティブ境界 | `audio10.rs::surround_wav_decodes_native_masks_and_preserves_channel_order`: 5.1(side)/5.1(back)/7.1 WAV が正しいマスク・チャンネル順・サンプルでデコードされ probe が `channel_mask` を報告。マスクなし 6ch は `UNSUPPORTED_CHANNEL_LAYOUT` |
| エンコード往復 | `pcm24_surround_encode_decode_roundtrip_preserves_mask_and_samples`: 5.1 bus → pcm_s24le mov → probe(6ch・mask 0x60f) → デコードが PCM24 精度で一致。mux への偽レイアウト宣言は型付き拒否 |
| エクスポート統合 | `export_av_multichannel_layout_reaches_the_mux_and_probe`: `with_audio_layout` がエンベロープ 3・スナップショット hash にレイアウトを含め、評価器の 5.1 ミックスと mux 出力が一致。`export_av_stereo_layout_is_the_documented_explicit_downmix`: stereo 出力が評価器の ITU ダウンミックスと一致 |
| ジョブ経路 | `crates/kronello-cli/tests/jobs.rs::surround_audio_layout_reaches_fixed_job_output_and_probe`: `render.submit`（pro_res_mov・audio_layout 1551・profile_version 3）が固定スナップショット経路で 6ch pcm mov を生成し、`verify_movie_layout` とデコード一致を確認。`multichannel_layout_on_legacy_audio_envelope_is_rejected_at_submit`: profile_version 1 への非 stereo 宣言は submit 時に `UNSUPPORTED_FEATURE` |

## 残件

- 実機での視聴覚確認（サラウンドモニタリング、可変速度の聴感品質）は行っていない。決定的アルゴリズムのため評価器レベルの一致検証で担保する。
- position ベースの任意レイアウト、22.2ch 等のクローズドセット外レイアウト、Ambisonics は対象外（ADR-0124）。
- ピッチ保持は `PitchPreserveV1` の正速度リニア/区分線形マップのみ。リバース・ネストしたコンポジションリタイムとの組合せは型付きエラーのまま。
