# AUDIO-008 ダイナミクスと LUFS 計測・ノーマライズの受け入れ記録

状態: `m8-lane-e` 作業ツリーで確認済み（2026-10-08）。実装は ADR-0117 の決定に従う。

## 実装範囲

- `EffectParameters` に v1 バリアントを追加（`crates/kronello-model/src/effect.rs`）。
  - `AudioCompressor { threshold_db, ratio, attack_ms, release_ms, makeup_db }`: `kronello.audio.compressor`。
  - `AudioLimiter { ceiling_db, release_ms }`: `kronello.audio.limiter`（`ceiling_db` は [-120, 0]）。
- `crates/kronello-audio/src/dsp.rs` のダイナミクス: ステレオリンクの feed-forward ピーク検出 + dB 域ゲイン計算（コンプレッサー）、瞬時アタック + 指数リリースの天井リミッター。フィルタと同じく係数・状態は決定的で、再生/書き出しは同一コード経路。ステートフルチェーンはプレースメント先頭から評価し、シーク・分割要求でも連続レンダーとサンプル一致する。
- `crates/kronello-audio/src/loudness.rs`: ITU-R BS.1770-4 の K 重み付け（2 段 biquad、48 kHz 固定係数）に基づく integrated（400 ms ブロック + 絶対 -70 LUFS / 相対 -10 LU の 2 段ゲート）・momentary（400 ms 最大値）・short-term（3 s 最大値）LUFS と 4x オーバーサンプル true peak（dBTP、32 タップ Blackman 窓 sinc を 4 位相に分割・各位相 DC 正規化）。完全決定的。
- `audio.loudness` クエリ（`crates/kronello-service/src/loudness.rs`）: 対象は `clip`（オーディオトラック上の 1 クリップを他トラックミュート+トランジション除去で孤立化し共有 DocumentAudioPlan で測定）/ `sequence`（全体または指定範囲）/ `asset`（検証済みストリームを直接デコード、範囲指定可）。`base_revision` チェック付きの読み取り専用クエリ。
- `audio.normalize` コマンド: 対象クリップを共有プランでレンダリングし integrated LUFS を測定、`target_lufs - measured` のゲインを `kronello.audio.gain` エフェクトとして `edit.plan`/`edit.apply` の `clip_set_effects` で 1 イベント追記する（Undo 可能・冪等キー・REVISON 競合も通常編集と同一）。`target_lufs` は (-70, 0]、静寂/短すぎるクリップは `INVALID_AUDIO_INPUT`。

## 受け入れ証拠

この作業ツリーで実行（2026-10-08）:

```
cargo test -p kronello-audio -p kronello-model -p kronello-service -p kronello-ffi --locked   # 全て成功
```

| 確認項目 | 内容 |
|---|---|
| コンプレッサー | `dsp::tests::compressor_reduces_hot_sine`: -20 dB threshold・4:1 で約 -10 dB ゲイン。`dynamics_process_chain_and_arbitrary_partitions_are_bit_identical`: ゲイン 4x 前段 + コンプで期待レンジに圧縮され、任意分割が連続ミックスと bit 一致 |
| リミッター | `limiter_holds_ceiling`: -6 dBFS 天井を保持。結合テストでも -3 dBFS 天井を保持し分割一致 |
| LUFS 既知レベル | `loudness::tests::sine_at_minus_20_dbtp_measures_about_minus_20_lufs`: 997 Hz・振幅 0.1 の両チャンネル正弦波が integrated ≈ -20.0 LUFS ±0.1、momentary/short-term 一致、true peak ≈ -20.0 dBTP ±0.2。静寂・短入力の部分報告・決定性も検証 |
| サービス計測 | `crates/kronello-service/tests/loudness.rs`: 0.25 振幅 tone440 クリップで integrated ≈ -12.7 LUFS、`clip`/`sequence`/`asset` 入力、REVISION_CONFLICT・ASSET_MISSING の型付きエラー |
| ノーマライズ + Undo | `normalize_appends_gain_then_undo_restores`: -20 LUFS 目標で `kronello.audio.gain` が 1 イベント追記され、計測 LUFS が目標 ±0.3 に一致、`edit.undo` で完全復元。`normalize_and_loudness_fail_typed`: 範囲外 target_lufs（`INVALID_REQUEST`）、静寂クリップ（`INVALID_AUDIO_INPUT`）、非オーディオクリップ（`ASSET_MISSING`） |
| スキーマ整合 | `tests/api.rs`: `audio.loudness`/`audio.normalize` が wire スキーマ・コマンドレジストリ・実結果スキーマと一致。capabilities に `audio_filters_v1`/`audio_dynamics_v1`/`audio_loudness_v1` と 5 つの新エフェクト ID を公開 |

## 残件

- `audio.loudness` の測定は共有プランでのレンダリングに基づくため、素材デコードは `MAX_AUDIO_FRAMES`（約 10 分）の共有バジェットに従う。
- ノーマライズはクリップ単位（既存エフェクト列の末尾にゲインを追記）。トラック/シーケンス単位のノーマライズは未実装。
- ノーマライズの UI は未実装（API/CLI 経路のみ）。
