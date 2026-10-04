# ADR-0051: Sequence の配置・合成順序と三つの時間編集を固定する

- 状態: 部分置換（[ADR-0062](0062-video-generator-and-timeline-edits.md): transition による明示 overlap と動画・Generator / clip effects / move・ripple・link の追加。その他の規約は維持）
- 日付: 2026-10-04
- 対象: NLE-001

## 背景

同じ Composition を異なる時刻・尺で配置しても、定義を複製したり評価履歴に依存したりしない。Timeline と Composition の分離、純粋評価、有理数時間、共有 Command / Query API、TEMPLATE-001 の保護区間、ADR-0049 の音声 Bus を維持する。既存 ADR は置換しない。0050 は並行する JOB-001 に予約されている。

## 決定

- `Project.sequences` は省略可能な UUID 集合。`Sequence` は extent、frame_rate、audio_rate、working_space と順序付き `Track`、`Track` は video / audio と `Clip` を持つ。Sequence / Track / Clip はそれぞれ安定 UUID を持つ。Composition Clip の実行キーは ClipId で配置を区別し、内部の NodeId と InstancePath を保つ。
- track 配列は下から上への source-over 合成順。同一 track の区間重複は `CLIP_OVERLAP`。別 track の重複は許可し、同一 track でも `[start,end)` の端点で接する配置は許可する。clip 配列の保存順で同時描画順を選ばない。transition / ripple は今回実装しない。
- `local = source_in + time_map(sequence_time - timeline_range.start)`。正の線形 / 厳密増加の区分線形 TimeMap を使い、配置の両端で source bounds / domain を検証する。domain 外の外挿や clamp はしない。
- `clip.trim` は既存配置内の非空区間へのカット。残す時刻の source 内容と速度を保ち、source_in と TimeMap 原点を更新する。`clip.stretch` は配置区間を置換し、source span / local control values を保って親時間だけを伸縮する。両操作とも Composition 定義と内部 instance map を変更しない。
- `instance.retime` は CompositionInstance の内部 `local_time_map` だけを変更し、node active_range、Clip 配置と source_in を保つ。map の domain と参照 Composition の duration を検証する。保護 template に到達する instance の汎用 retime、保護内容を含む clip の stretch は `PROTECTED_INTERVAL`。`template_instance.retime` は既存の duration policy / `template.set_duration` を使い、intro / outro を保って中間だけを伸縮する。最低尺未満は `DURATION_TOO_SHORT`。
- `sequence.create` / `clip.place` / `clip.trim` / `clip.stretch` / `instance.retime` / `template_instance.retime` は共通 service registry、schema、plan / apply / receipt / selective Undo を使う。Sequence collection は UUID member patch、tracks / clips は順序付き配列として一括置換し、同じ Sequence の構造編集は保守的に競合させる。
- `RenderTarget` は Composition / Sequence を明示する。既存 `composition` 入力も保持し、`target` と同時指定・未指定は拒否する。Sequence は独立 instance を持つ実行用 Composition へ lower し、既存 Scene IR / DAG / 明示 backend を使う。保存文書に合成用 Composition を追加しない。snapshot は Sequence、配置、資産、意味版、revision を固定し、Sequence の working_space と profile の一致を検証する。選択した opaque Sequence は `UNSUPPORTED_FEATURE`。
- 音声は明示 audio track の Asset / stream index、unity-speed の線形 map だけを 48 kHz stereo Bus へ接続する。絶対 sample floor と source_in、Gain::UNITY を使い、複数 track をミックスする。retimed audio / audio effects / generator audio は型付き拒否。Composition の音声再帰、clip volume 編集、映像 Asset / Generator Clip の描画、Sequence A/V mux は今回の範囲外。画像レンダーは音声を生成せず、`mix_sequence_audio` は固定 Project 値を明示入力とする。

## 影響

暗黙の overlap 順序、trim と stretch の兼用、Composition 定義の尺変更による全配置への波及は採用しない。既存 Composition レンダーとの互換性を保ちながら、同じ定義の独立した配置を評価・CPU 画素・永続化で検証できる。GPU 検証は sandbox の受け入れ証拠に含めず、supervisor の host run に残す。

## 関連

- [01 データモデル](../architecture/01-data-model.md)、[02 時間](../architecture/02-time.md)、[05 レンダラー](../architecture/05-render-gpu.md)、[08 API](../architecture/08-api-cli-mcp.md)
- [ADR-0003](0003-pure-evaluation-at-arbitrary-time.md)、[ADR-0043](0043-semantic-dependencies-and-units.md)、[ADR-0045](0045-snapshot-compatibility-boundaries.md)、[ADR-0049](0049-audio-bus-timing-and-codec.md)
- [NLE-001 の検証](../testing/nle-001.md)
