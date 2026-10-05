# ADR-0063: 文書音声の配置・音量 Property と明示的な出力選択

- 状態: 部分置換（ADR-0069、下記の追加範囲のみ）
- AUDIO-004 の追加・部分置換: [ADR-0069](0069-versioned-stateless-audio.md)。audio track の Generator / effect Property / crossfade と明示 resample_v1 を追加し、movie profile 1/2 の実行意味は維持する。
- 日付: 2026-10-05
- 対象: AUDIO-003

## 背景

AUDIO-000 / JOB-001 は固定 RenderSnapshot と明示音声 clips の ProRes / PCM24 MOV を提供した。
Sequence audio track と Composition の再帰音声は未接続だった。M2 の `clips: []` は silence なので、
これを自動文書音声へ変更すると既存要求・固定入力の意味が変わる。

## 決定

### 一つの固定入力と出力 mode

- `JobOutput::ProResMov` に `profile_version` と `audio: document | explicit | silence` を追加する。
  省略時は version 1 / explicit。version 1 は explicit のみ、version 2 は三 mode に対応する。
  未知 version、version 1 の document / silence は `UNSUPPORTED_FEATURE`。
- document は選択した Sequence / Composition からだけ音声をコンパイルする。explicit は
  `output.clips` のみを使い、空配列は従来どおり silence。silence は意図的な無音。
  document / silence と非空 clips の併用は `INVALID_MEDIA_INPUT`。自動的な加算・切替はしない。
- 同期 `render.export` と `render.submit` は同じ `RenderSubmitRequest`、`freeze_render_input`、
  `movie_snapshot`、`MediaRuntime::export_av` を使う。同期は MOV のみ、image_sequence は既存
  `render.sequence` を使う。背景と clipping Reject、SDR / memory budget / atomic publication は維持する。
- RenderSnapshot は映像・音声の Project / revision / asset lock / Curve を所有する。
  `AvExportSnapshot::new` は既存 schema 1 の explicit envelope を維持し、
  `with_audio` は schema 2 に mode と文書由来の確定配置を保存する。復元時も同じ文書から
  配置を再計算して照合する。音量 Property / Curve は RenderSnapshot hash、mode と配置は
  export hash、要求の mode / profile version は job input の byte SHA-256 に含まれる。
  最新作品を読み直さない。report は `audio_source` / `audio_profile_version` と両 snapshot hash を返す。

### 文書モデルと Gain

- `NodeKind::Media(MediaNode)` は AssetId、明示 stream_index、非負 source_in、有理数 TimeMap、
  同じ SceneNode が所有する volume PropertyId を持つ。media map の親時刻は
  `composition_time - node.active_range.start`。AUDIO-003 は音声だけを実行する。
  Audio asset の Media は描画内容を持たない。Video / Image asset の Media の描画は
  `UNSUPPORTED_FEATURE` とし、COMP-002 まで placeholder を描かない。
- descriptor `kronello.audio.volume` は dimensionless Scalar。0 は mute、1 は unity、有限・非負で
  f32 Gain の範囲内。定数と Scalar Curve（Hold / Linear / Cubic）に対応する。
  Expression / Modifier は descriptor が拒否する。Curve の補間後にも Gain の範囲を検証する。
  負値・欠落 / 空 / 型不一致 Curve は `INVALID_AUDIO_INPUT`、未知補間版は `UNSUPPORTED_FEATURE`。
- Clip に optional `volume: Property` を追加する。省略 / null は unity、既存保存内容に
  ランダムな PropertyId を補わない。共有 `edit.plan` / `edit.apply` の
  `timeline.clip_set_volume { sequence, clip, volume }` で設定・解除する。
  revision / idempotency / selective Undo は既存 transaction を使う。Sequence と Clip の構造 key
  で保守的に競合させる。Clip Curve は source-local time、Media Curve は Composition-local time。

### 再帰配置・区間・mix 順

- Sequence は authored track 順、各 track の clips 配列順に加算する。Audio track の Asset / Composition
  に加え、Video track の CompositionClip は参照 Composition の音声を一度だけ継承する。
  Video track の直接 Asset から別 stream を推測して加算しない。
- Composition は root_nodes / child_order の順で深さ優先。CompositionInstance の参照 roots は
  その node の authored children より先に展開する（既存 scene traversal と同じ）。
  nodes の保存配列順に依存しない。同一 definition の別 placement は別の有理数 offset / active 区間を持つ。
- 音声を持つ経路の map は unity-speed Linear のみ。
  Clip: `local = source_in + map(sequence_time - timeline_range.start)`。
  Instance: `child_local = map(parent_local)`。Media: `source = source_in + map(local - active_range.start)`。
  offset を checked 有理数で合成し、すべての祖先 active_range と Composition の `[0,duration)` を交差する。
  現行 SceneNode に enabled / mute field はないので、active_range のみで有効性を決める。
  opacity / transform は音量へ転用しない。
- 文書音声は合成済み affine 写像 `source_time = output_time + delta` の逆写像を一回 floor し、
  `destination_origin = floor(-delta × 48000)`、
  `source_sample = absolute_output_sample - destination_origin` とする。
  source sample の正確な時刻を出力の絶対 floor 格子へ配置するため、NTSC 境界から始まる
  source_in=0 の素材へ負の pre-roll を要求しない。
  effective placement の `floor(start×48000)..floor(end×48000)` を使い、flattened AudioClip の
  source_in はこの最初の整数 source sample を表す正確な有理数にする。元の source 原点と
  祖先 trim を独立に保持するので、fractional trim 前後で sample phase が変わらない。
  source sample が負なら `AUDIO_SOURCE_TOO_SHORT`。pre-roll を無音で代替しない。
  AUDIO-000 の explicit clips は従来の二境界 floor の規則を維持する。
- 音量は各整数 sample の `time = index / 48000` から純粋評価し、Clip Gain × Media Gain を
  f32 で適用して配置順に加算する。丸め済み frame 長や浮動小数点の時刻を積算しない。
  compiled plan は Property と使用 Curve を所有し、compile 後の編集で評価値を変えない。
  最終 stream は zero-origin、A/V duration 差は厳密に 1/48000 秒未満。

### 未対応と失敗

- retimed audio、音声経路の effects、Generator 音声は `UNSUPPORTED_FEATURE`（AUDIO-004）。
- NLE-002 の crossfade は画素だけを混ぜる。音声を持つ clip が transition に含まれる場合、両 placement を unity で加算せず `UNSUPPORTED_FEATURE` にする（音声 crossfade は AUDIO-004）。Generator は NLE-002 の Sequence 検証で video track に限られるため、audio track の Generator は `INVALID_CLIP` になる。
  音声を持たない Composition の映像 retime は既存経路を維持する。
- 48 kHz stereo、1024 placements、64 nested scopes、100000 nodes / scope と既存 source / Bus memory budget。
  循環・重複 containment・budget 超過は `INVALID_AUDIO_INPUT`。
- decode 前後の asset hash 検証、欠落・不足・hash 不一致・clipping の型付き失敗と出力の
  no-clobber / job lease fence を維持する。mute は asset 検証を省略する理由にしない。

## 影響と検証

AUDIO-000 / JOB-001 の明示入力の意味、ADR-0026 の Undo、ADR-0049 の Bus / PCM24 / mux、
ADR-0051 の trim / stretch は維持する。文書音声と音量の追加範囲を本 ADR で定める。
リアルタイム callback、動画 Media 描画、retime / effects / Generator、長尺 streaming は後続。
検証結果と host に残す範囲は [AUDIO-003](../testing/audio-003.md) を参照。
