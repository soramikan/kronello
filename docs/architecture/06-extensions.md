# 06 拡張点: Repeater・Simulation・音声・3D

基本エフェクトは FX-001 / FX-002、基本音声は M2〜M3、音声連動は AUDIO-001 で受け入れ済み。Repeater は REPEAT-001 の共有 Source と明示 expand の受け入れを完了した。最終受け入れはバックログと検証記録を正本とする。Simulation は SIM-001 の実装・検証中、2.5D は後続タスクの設計である。

## 版付きエフェクト

基本エフェクト（FX-001 / FX-002）は `SceneNode.effects` に authored 順で保存する。`EffectDefinition` の `effect_id` は `kronello.gaussian_blur` / `kronello.drop_shadow`、`version` はそれぞれ **1 / 2**。1 は旧 separable 意味を固定し、2 は非一様 affine の elliptical kernel を明示選択する（[ADR-0067](../adr/0067-affine-gaussian-effects.md)）。parameters は型付きの PropertyId 参照で、局所 sigma / offset は `design_px`、色はタグ付き straight、opacity は無次元。未知 id・版・parameter variant・field を保存しても実行能力とは扱わず、必要な未知 effect は最終レンダーで `UNSUPPORTED_FEATURE`。

`ResolvedEffect` → `PixelEffect` → DAG effect / GPU pass の境界に具象 GPU 資源を漏らさない。`required_input` は effect の halo を宣言し、stack の逆順で伝播する。`RenderSnapshot.semantic_versions.effects` は id ごとの対応版上限（新規 2 / 旧 1）を固定し、各 authored version が実際の意味を選ぶ。実際の kernel 版を含む cache identity を使う。処理順・変換制約・ROI・kernel 定義は [05 章](05-render-gpu.md#基本エフェクト)、受け入れ証拠は [FX-001](../testing/fx-001.md) / [FX-002](../testing/fx-002.md) を参照。

## Repeater

`Project.repeaters` の版 1 record を `NodeKind::Repeater { content_ref }` が参照する。Source は共有 Composition の単一 root を明示し、root Group が任意の通常 subtree を保持できる。既存の複数 root や外部 parent を持つ部分木は自動コピー・切断せず、明示した Group 構成がなければ `REPEATER_SOURCE` で拒否する。

保存済みの instance ID / placement ID / seed、Property / Effect、enabled / active_range、rational local_time_map、Composition input bindings を保持する。instance の配列順は描画順だけを表し、乱数 identity にしない。通常 compile は共有 Source を参照する CompositionInstance へ純粋に lowering するため、個数分の Source 編集オブジェクトを保存しない。描画のまとめ方は Blend / Mask / Effect で変わり、単一 draw call を保証しない。

通常 Property 操作は保存済み placement ID を指定し、個別の Source 編集だけを共有 `RepeaterExpand` Command で明示する。expand は Source / TemplateDefinition / TemplateInstance を変更せず、指定 instance の独立 Source を作る。ID と seed は維持し、nested Noise 座標は alias により維持する。nested TemplateInstance は選択 variant、公開 text / Property / Media / DataTable 入力をコピーへ materialize し、時間写像と動的 band / max-lines 規則を保持する。保存・Undo・競合処理は共有 edit transaction を使う。

snapshot は `semantic_versions.repeater = 1` を固定し、必要な未知版、missing Source、cycle、不正 identity / layout を型付きエラーにする。詳細は [ADR-0103](../adr/0103-shared-repeater-and-explicit-materialization.md)、受け入れ証拠と残件は [REPEAT-001](../testing/repeat-001.md) を参照。

## Simulation

通常のアニメーションは `value = f(snapshot, time, instance)`。
状態を要する粒子等だけ `state(k+1) = step(state(k), inputs(k))`。

通常 Property の純粋評価・依存境界は [ADR-0043](../adr/0043-semantic-dependencies-and-units.md) に従う。Simulation の状態・アルゴリズム版は snapshot の `semantic_versions` に固定し、未対応の必要機能は最終レンダーで拒否する（[ADR-0045](../adr/0045-snapshot-compatibility-boundaries.md)）。

SIM-001 の採用設計では、actual playback mapping が表示したい source 時刻を選び、physics は canonical forward authored source clock の固定 tick で通常 Property 入力を評価する。protected loop / hold は同じ source 時刻の forward state を再利用し、reverse は forward state を逆順に参照する。入力 closure / clock transform / algorithm version を checkpoint identity に含める。詳細は採用 [ADR-0104](../adr/0104-canonical-source-clock-simulation-checkpoints.md)。モデルと純粋カーネルを実装し共有レンダーへ統合中であり、受け入れは未完了である。

- 固定刻み、固定 seed、版付き状態、入力ハッシュ、チェックポイントを使う。
- シーク時は必要な時刻より前の有効な checkpoint から再計算する。
- 逆再生は元の正方向シミュレーション時刻を参照し、数値積分を逆方向に巻き戻さない。
- 同じ環境内の結果一致を検証し、GPU 原子演算等を含む完全なクロスデバイス決定性を別問題として扱う。
- 分散レンダーは未ベイク Simulation を含む区間を自由分割しない。

## 音声

### 基本音声（M2〜M3）

- M2（AUDIO-000）: 素材音声のデコード、48kHz への変換、クリップ音量、Bus へのミックス、音声付き書き出し。サンプル位置は絶対時刻から計算し、映像との同期をサンプル精度で検証する。
- M3（AUDIO-002）: GUI でのリアルタイム再生と A/V 同期。音声コールバックはプロジェクト更新・ディスク読み出し・式評価と別の実行系にする。

### 音声連動（M5）

AUDIO-001 は `done`。不変の `Project.audio_analyses`、共有 `audio.analyze`、Expression 版 2 の `AudioFeature` 参照を実装し、固有条件と統合 checkpoint の受け入れを完了した。解析契約は[ADR-0096](../adr/0096-offline-audio-feature-assets.md)、実行済みテストと残件は[検証記録](../testing/audio-001.md)を参照する。版1のbeatは閾値を超えたonset pulseであり、tempo・拍子推定ではない。

- RMS、帯域エネルギー、onset / beat 等を事前解析し、タイムスタンプ付き DataAsset として保存する。
- Asset hash、サンプルレート、窓長、ホップ長、解析版、time-map を固定する。
- 毎描画フレームで音声全体を再解析しない。
- ミックス後音声の特徴量を使う場合は対象 Bus と mix snapshot を固定する。
- 声の速度を自動変更せず、テンプレート SE は intro / outro marker へ配置する方式を優先する。

## 2.5D / 3D

- 当初は 2D。次に平面、奥行き、カメラの 2.5D を Scene3D サブグラフで実装する。
- 2D の描画順と 3D の depth / transparency 処理を同一の z 値だけで統一しない。
- 将来の glTF、キャラクター等は明示した 3D 境界から取り込み、外部レンダラーの色・alpha・必要な補助チャンネルを合成できるようにする。
- フル 3D、リグ、物理、パストレーサーを動画編集 MVP の必須条件にしない。

外部レンダーの Color 入力にも [ADR-0044](../adr/0044-color-and-alpha-contracts.md) の色空間・alpha 表現・変換順を適用する。3D の座標軸・変換型の詳細は本段階では固定しない。補助チャンネルや未知ノードを保持できることと、描画できることは区別する。
