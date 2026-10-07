# AUDIO-007 フィルタ系音声エフェクト（パラメトリック EQ・HPF/LPF）の受け入れ記録

状態: `m8-lane-e` 作業ツリーで確認済み（2026-10-08）。実装は ADR-0117 の決定に従う。

## 実装範囲

- `EffectParameters` に v1 バリアントを追加した（`crates/kronello-model/src/effect.rs`）。
  - `AudioEq { bands }`: `kronello.audio.eq`。`bands` は DataTable Property で、列は `kind`（Enum: `peak` / `low_shelf` / `high_shelf`）・`freq_hz`・`gain_db`・`q` の 4 列、1..=8 行。
  - `AudioHpf { cutoff_hz, order }`: `kronello.audio.hpf`。`order` は 1..=4。
  - `AudioLpf { cutoff_hz, order }`: `kronello.audio.lpf`。同上。
- `ensure_supported` / `references` / 新規 `resolve_audio` を拡張し、パラメータは PropertyId 参照・値型・範囲（`cutoff_hz` は 0 より大きく 24 kHz 未満、`order` は整数 1..=4、EQ 行の kind/周波数/ゲイン/Q 範囲）をすべて型付きエラーで検証する。汎用の映像 `resolve` は検証後に `UnsupportedFeature` を返す（音声評価器専用であることを明示）。
- 記述子を `effect_descriptors()` に追加（`eq_bands` / `cutoff_hz` / `order`、ID は `0xf0000000_0010_450x` 系）。
- DSP は `crates/kronello-audio/src/dsp.rs`: RBJ クックブックの peaking / shelf biquad と Butterworth 1〜4 次カスケード（奇数次は一次セクションを追加）。係数は定数パラメータから f64 で決定的に計算し、状態は毎回ゼロ初期化される。
- `crates/kronello-audio/src/advanced.rs` の clip effects 受理集合を拡張。`kronello.audio.gain` と同じ `clip_set_effects` の流れに乗る。ステートフルなチェーンはプレースメント先頭から評価し、要求範囲が分割されても連続ミックスと完全一致する（リアルタイム再生と書き出しは同一コード経路）。
- 非 Constant（アニメーション）パラメータ・オーディオトラック以外への適用・未対応エフェクトは `UNSUPPORTED_FEATURE` / `INVALID_AUDIO_INPUT` の型付きエラー。

## 受け入れ証拠

この作業ツリーで実行（2026-10-08）:

```
cargo test -p kronello-audio --locked      # dsp モジュール内 6 件 + tests/dsp_effects.rs 3 件を含み全て成功
cargo test -p kronello-model --locked     # 全て成功（エフェクト解決・スキーマ検証を含む）
cargo clippy -p kronello-model -p kronello-audio -p kronello-service -p kronello-ffi --all-targets --locked -- -D warnings
cargo fmt --all --check
```

| 確認項目 | 内容 |
|---|---|
| EQ の帯域特性 | `dsp::tests::peaking_eq_boosts_band_and_leaves_far_signal`: 1 kHz +12 dB ピーク EQ が帯域内で約 +12 dB、遠い 100 Hz 正弦波をほぼ不変に保つ |
| HPF/LPF の減衰 | `hpf_attenuates_below_cutoff`（4 次 2 kHz HPF が 100 Hz を 1/100 以下に）、`first_and_third_order_build`（1/3 次カスケード） |
| 再生=書き出しの一致 | `dsp_effects.rs::filters_process_chain_and_arbitrary_partitions_are_bit_identical`: 共有 `DocumentAudioPlan` でのフィルタ処理が減衰・ゲインを満たし、要求範囲の任意分割が連続ミックスと bit 一致（リアルタイム=書き出しの同一経路証明） |
| 型付きパラメータエラー | `audio_effects_require_audio_tracks_and_typed_parameters`: 範囲外 cutoff/order（`INVALID_AUDIO_INPUT`）、欠落 Property、非 Constant source（`UNSUPPORTED_FEATURE`）、不正 EQ テーブル、映像トラック・非音声エフェクトの拒否 |

## 残件

- フィルタ/ダイナミクスパラメータは Constant 限定。アニメーション可能なオーディオエフェクトは別タスク。
- GUI からのオーサリング UI は未整備（`clip_set_effects` API 経由では設定可能）。
