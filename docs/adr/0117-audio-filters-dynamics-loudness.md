# ADR-0117: 音声フィルタ・ダイナミクス・LUFS 計測と再生メーター

- 状態: 採用
- 日付: 2026-10-08

## 背景

AUDIO-007 はパラメトリック EQ / HPF / LPF、AUDIO-008 は
compressor / limiter と LUFS 計測・ノーマライズ、
AUDIO-009 は VU メーター・オーディオスクラブ・ミキサー
パネルを要求する。AUDIO-004 で clip effects の枠組み
（`kronello.audio.gain`）は既にあり、これを拡張する形で
フィルタ・ダイナミクス・計測 API を確定する。

## 決定

### 音声エフェクト（`EffectParameters`、いずれも v1）

- `kronello.audio.eq` — `bands`（DataTable: `kind`（peak|
  low_shelf|high_shelf）, `freq_hz`, `gain_db`, `q`、1..=8 行）。
- `kronello.audio.hpf` / `kronello.audio.lpf` — `cutoff_hz`,
  `order`（1..=4）。
- `kronello.audio.compressor` — `threshold_db`, `ratio`,
  `attack_ms`, `release_ms`, `makeup_db`。
- `kronello.audio.limiter` — `ceiling_db`, `release_ms`。

フィルタは biquad、ダイナミクスはレベル検出 + ゲイン
コンピュータで、両者とも deterministic に実装する。
リアルタイム再生と書き出しで**同一実装**を使い、
サンプル一致を検証する（許容差ではなく同一コード経路）。

### LUFS 計測とノーマライズ

- `audio.loudness` クエリ：対象（asset / sequence / 範囲）に
  対し ITU-R BS.1770 の K 重み付き integrated / momentary /
  short-term LUFS と true peak（dBTP）を返す。
- `audio.normalize` コマンド：対象クリップに対し
  目標 LUFS に合わせた `kronello.audio.gain` を設定する
  （`edit.apply` の mutation として Undo 可能）。

### 再生メーター（AUDIO-009 の API 側）

- 再生中のトラック別 peak/RMS を周期 publish する FFI 経路を
  追加する（既存の `RealTimePlayback` 系ブロックにメーター
  値を載せる）。GUI はこの値を VU メーターと Mixer パネルに
  表示する。オーディオスクラブは既存の再生パイプラインの
  短区間再生として実装し、専用の新規評価経路は作らない。

## 影響

- 音声エフェクトは `clip_set_effects` で既存の流れに乗る。
- フィルタ・ダイナミクスの状態は再生/書き出しの両方で
  deterministic に初期化され、シーク時も同一結果を返す。
- LUFS はメートル法規格に基づく決定的計測として実装する。

## 関連

- AUDIO-007、AUDIO-008、AUDIO-009、ADR-0049
  （音声バス・タイミング）、`crates/kronello-audio/src/advanced.rs`
