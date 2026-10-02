# 動画編集・モーショングラフィックス共通基盤 設計仕様 v0.2

作成日: 2026-10-01
状態: 提案仕様。ソフトウェアの実装・実機性能検証を完了したものではない。
対象: Rust / FFmpeg / wgpuを基盤とする、GUI・CLI・MCP共通の動画編集ソフトウェア。

## 0. 結論とスコープ

NLE（カット編集）のTimelineと、モーショングラフィックスのCompositionを別の編集モデルとし、同じ時間・プロパティ・組版・合成・レンダー基盤へコンパイルする。
CompositionはSourceRefとしてTimelineへ配置できる。Compositionの中から別のCompositionも参照できるが、参照循環は禁止する。

初期リリースから実装する基盤:
- 有理数時間、サブフレーム評価、InstancePath、ローカル時間マッピング。
- 型付きProperty、基本キーフレーム、依存関係検証。
- 2DのComposition、Shape、Text、Group、Null、Media、CompositionInstance。
- 幾何形状と組版結果を保持する中間表現。
- 空間領域・時間サンプルを要求できるレンダーAPI。
- 公開入力を持つテンプレート、変更計画・トランザクション・プレビュー検査API。

後段で実装する機能:
- 高度な式、Path morph、Trim path、Repeater、音声連動、ルビ・縦書きの完全な編集UI。
- 多重サンプルの高品質モーションブラー、チェックポイント付きシミュレーション。
- 2.5Dカメラ、完全な3D、外部プラグイン互換、分散レンダー。

初期から3D描画を実装するのではなく、出力ポート・時間・変換型・拡張バージョンの境界を確保する。
初期未対応の機能を保存形式に記録しても、「対応済み」と表示したり、無視して最終レンダーしたりしない。

## 1. アーキテクチャ決定

| ID | 決定 | 理由・制約 |
|---|---|---|
| ADR-001 | GUI/CLI/MCPは同じCommand/Query APIを使う | GUI専用の作品状態を作らない |
| ADR-002 | TimelineとCompositionは別モデル、共通IRへ変換 | カット編集のリップルと空間階層を混ぜない |
| ADR-003 | 通常アニメーションは任意時刻の純粋評価 | シーク・逆順レンダー・再試行を同じ結果にする |
| ADR-004 | Propertyの主値源はConstant/Curve/Expressionのいずれか | 複数の値源が暗黙に上書きしない |
| ADR-005 | 意味的スナップショットとGPU資源を分離 | バックエンド、GUI、GPU資源寿命に文書が依存しない |
| ADR-006 | 文書保存とレンダーキャッシュを分離 | キャッシュの削除で作品データを失わない |
| ADR-007 | テンプレート定義とインスタンス入力を分離 | 一箇所の変更で全配置を意図せず書き換えない |
| ADR-008 | CPU/GPU転送経路を明示する | GPU内コピーとCPU往復を区別する |
| ADR-009 | 式評価にネットワーク・ファイル・時計・非固定乱数を与えない | 再現性、キャッシュ、安全性 |
| ADR-010 | 未対応機能は保持可能でも最終出力は失敗させる | 黙った画質低下や欠落を防ぐ |
| ADR-011 | 初期の正本はローカルSQLite | DB/イベント/現在状態を同一トランザクションで更新 |
| ADR-012 | HDR映像合成とベクターラスタライザーを分離 | Rgba8Unorm前提の描画部にHDR全体を通さない |

## 2. 共通実行経路

```text
GUI / CLI / MCP
       |
Command API / Query API
       |
Project Service (single writer, revision, policy, audit)
       |
Immutable Snapshot
       |
Document Compiler
  |- Timeline -> placement model
  |- Composition -> scene model
  |- Property / Layout -> dependency graph
  |- Template -> instance bindings
       |
Scene IR + Render DAG
       |
Time/Region Scheduler + Cache + Resource Budget
       |
Text/Vector raster / Video decode / GPU effects / Audio mixer
       |
Preview / Still image / Image sequence / Encoder
```

リアルタイム音声コールバックとプロジェクト更新、ディスク読み出し、重い式評価は別の実行系にする。
素材分析（ASR、音声特徴量、人物マスク等）は不変のDataAssetを生成する外部ジョブとして取り込む。

### 2.1 関係グラフを混同しない

| 構造 | 役割 | 不変条件 |
|---|---|---|
| Timeline placement | クリップ配置、トリム、リップル、リンク | 時間区間と対象トラックを明示 |
| Scene containment | 描画順、Group隔離、所有関係 | 一つの所有親、循環なし |
| Transform parenting | 親の変換を継承 | 描画順・所有親とは独立、循環なし |
| Property/Layout dependencies | 値、式、テキスト計測、制約 | 依存を静的に列挙、循環診断 |
| Render DAG | 色・マスク・画像の処理順序 | 色空間、アルファ、時刻、領域が型付けされる |

ステートフルなSimulationは通常のProperty DAGに自己参照を埋め込まず、専用ノードと状態管理を使う。

## 3. データモデル

### 3.1 最小オブジェクト

| オブジェクト | 主なフィールド |
|---|---|
| Project | schema_version, semantic_version, assets, sequences, compositions, templates |
| Asset | id, content_hash, kind, stream_metadata, immutable_locator |
| DataAsset | id, schema, content_hash, values, time_mapping, analyzer_version |
| Sequence | id, extent, frame_rate, audio_rate, working_space, tracks |
| Clip | id, source_ref, timeline_range, source_in, time_map, links, effects |
| Composition | id, duration, design_extent, edit_rate, root_nodes, properties, inputs, markers, output_ports |
| SceneNode | id, kind, containment_parent, transform_parent, child_order, active_range, transform_ref, content_ref |
| CompositionInstance | id, definition_ref, input_bindings, local_time_map, seed |
| Property | id, type, units, source, modifiers, validation, capabilities |
| AnimationCurve | id, value_type, keys, interpolation_version |
| TemplateDefinition | id, version, composition_ref, public_inputs, duration_policy, constraints |
| RenderSnapshot | content_hash, revision, asset/font/data locks, semantic_versions, profile |

SourceRefはAsset、Composition、Generatorを区別する。SourceRefの型が増えてもClipの編集意味は変えない。

初期のSceneNode種類はGroup、Null、Shape、Text、Media、CompositionInstanceとする。
Mask/Matteは入力参照として表現でき、見えるレイヤーとして重複描画しない。Repeater/Particles/Scene3Dは拡張種類とする。

### 3.2 IDとインスタンス

NodeIdやPropertyIdを配列番号や名前から導出しない。表示名の変更で参照は変わらない。
同じCompositionを複数回使うため、実行時の参照キーは概ね `(InstancePath, NodeId, PropertyId)` とする。
InstancePathは、親からたどったCompositionInstanceの安定ID列であり、配列の現在位置ではない。
共有定義を編集する操作と、公開入力を上書きする操作を別APIにする。

### 3.3 保存と互換性

SQLiteの現在状態、イベント、逆操作情報、リビジョンを一つのトランザクションで更新する。
JSONは不変スナップショットまたはインポート形式であり、SQLiteと並行して書き換える第二の正本にしない。
イベントだけを唯一の保存形式にすると過去コマンドの意味変更が問題になるため、バージョン付き完全スナップショットも保持する。

`schema_version`は構造、`semantic_version`は補間・合成などの意味、各effect/templateのversionは実装依存を区別する。
未知の機能は保存時に失わない設計にするが、必要な機能が不足している場合の最終レンダーは`UNSUPPORTED_FEATURE`で拒否する。

## 4. 時間の規約

- 正本の時刻は正規化された有理数。整数演算はcheckedとし、中間計算は必要に応じてi128を使う。
- JSON上の分子・分母は10進文字列とし、JavaScriptの整数精度の制限を受けない。
- 区間は原則 `[start, end)`。
- フレームレートは30000/1001のように正確に保持する。
- 編集用フレームグリッドと評価時刻は別。フレーム間でもPropertyを評価できる。
- 音声のサンプル位置は絶対時刻から計算し、映像フレームごとの丸め誤差を累積させない。

```json
{"time":{"num":"1","den":"60"}}
```

### 4.1 時間階層

```text
Sequence time
 -> clip placement offset
 -> clip TimeMap
 -> Composition local time
 -> nested instance TimeMap
 -> animation local time
 -> media PTS / feature-data time
```

基本写像は `local = source_in + time_map(parent_time - placement_start)`。
TimeMapには後から区分線形、逆再生、ループ、停止、非線形を追加する。初期は線形と区分線形を実装する。
非線形写像は浮動小数点計算や求根を伴うため、完全に有理数だけで解けるとは扱わない。量子化精度・丸め・評価アルゴリズムの版を固定する。

Compositionは既定で親の連続時刻で評価する。編集レートが24fpsでも60fps出力時に整数の24fpsフレームへ勝手に丸めない。
コマ撮りのように内部レートを保持したい場合だけ、明示的なposterize/holdサンプリングを指定する。
動画素材の保持・補間・オプティカルフローは別の機能であり、ベクターの連続時間評価と混同しない。

### 4.2 トリムと長さ変更

`clip.trim`、`clip.stretch`、`template_instance.retime`は別操作とする。
尺の変更によって、保護されたイントロ・アウトロを黙って伸縮しない。
音声のリタイム方針も別に宣言する。複雑な非単調TimeMapに対する音声処理が未対応なら検証で拒否する。

## 5. プロパティ・アニメーション

```text
PropertySource<T> = Constant(T) | Curve(CurveId) | Expression(ExpressionId)
Property<T> = Source + ordered Modifiers<T>
```

位置、回転、スケール、不透明度、色、線幅、マスク、エフェクト、公開入力は同じProperty基盤に乗せる。
編集時の変換は2Dで `T(position) * R(rotation) * K(skew) * S(scale) * T(-anchor)` と定義する。
親子付け変更では、ローカル変換を維持するか、画面上の見た目を維持するかを操作で指定する。

| 型 | 補間規約 |
|---|---|
| Scalar/Vec2/Vec3 | Hold / Linear / Cubic |
| Angle | 巻き戻さない連続角。0→720度の2回転を保持 |
| Color | 補間色空間を明示。既定は作業用線形空間 |
| Bool/Enum/String/AssetRef | 離散切り替えのみ |
| Path | 点数・セグメント型・対応が一致する場合のみmorph |
| Transform3D | 将来の独立型。Quaternion等の意味を版管理 |

時間方向のイージングと空間的な移動パスを分ける。ベジェ曲線の時間ハンドルは時刻方向の単調性を検証する。
キーフレームの同時刻重複は暗黙に許容せず、upsert/replaceを操作として選ばせる。

### 5.1 式

最初は型付きASTと許可された組み込み関数で実装する。人間向けDSLは後から同じASTへ変換する。
演算、clamp、lerp、周期関数、固定seed noise、upstream Property参照、版固定DataAsset参照を対象にする。
ネットワーク、ファイル、現在時刻、環境変数、非固定乱数、無制限ループ、動的ノード探索を与えない。
式の参照先・サンプル時刻数・ノード数・命令数に予算を設ける。

`random(seed, instance_id, element_id)` は評価呼び出し順に依存させない。
時間変化するnoiseは時刻を明示引数とする。異なるGPU/CPU間の浮動小数点まで無条件にビット一致すると約束しない。

通常式からの再帰的な自己参照は禁止する。以前の値を積み上げる表現はSimulationへ移す。
失敗時に最終出力で勝手に基底値へ置換しない。プレビューの代替表示は警告付きにし、最終出力はエラーにする。

## 6. ベクター・日本語テキスト・レイアウト

### 6.1 ベクター

Path、Fill、Stroke、Gradient、ClipPathを意味的IRに保持する。最初からビットマップ化して保存しない。
幾何演算候補はkurbo、テッセレーション候補はlyon、SVG読み込み候補はusvg。これらの完全なSVG互換やあらゆるPath演算を前提にしない。
SVG読み込みは対応表を持つ。外部URL、script、外部フォント等は自動取得・実行せず、明示インポートする。

初期から矩形、角丸矩形、楕円、ベジェパス、単色塗り・線・基本グラデーションを扱う。
Trim path、線端・破線のアニメーション、Path boolean、morphは段階実装する。

Compositionのdesign_extentと出力画素数を分ける。同じ16:9で解像度だけを変える場合は原則再レイアウトしない。
16:9→9:16等のアスペクト比変更は、明示したresponsive variant/constraintで再レイアウトする。

### 6.2 日本語

```text
UTF-8 + style spans + ruby associations
 -> grapheme / shaping clusters
 -> glyph selection, metrics, Japanese line breaking
 -> LayoutResult (lines, glyphs, bounds, anchors)
 -> animation units / selectors
 -> glyph coverage / paths
 -> linear-HDR compositing
```

Parley/Fontiqueを基礎候補とするが、禁則、ルビ、縦書きの要件充足は個別に検証・補完する。
Unicodeの書記素クラスタとグリフは一対一ではない。元テキスト範囲から組版クラスタ・グリフへの対応を保持する。
「一文字ずつ」は既定で組版クラスタを壊さないAnimationUnitに変換する。
ルビ付き文字は親文字＋ルビを一体の単位にする既定動作を用意し、独立演出は明示指定とする。
セレクターは文字、行、語、範囲、タグに対応するが、語分割の辞書・アルゴリズムを版管理する。
テキスト更新で範囲指定が無効になった場合は再計算を報告し、曖昧な古いグリフ番号をそのまま使用しない。

### 6.3 レイアウトと描画の分離

Position/Opacityの変更では原則組版を再実行しない。本文、フォント、サイズ、折り返し幅の変更は組版キャッシュを無効化する。
語数・文字数が変わる型送りでは、原則として全文を組版してから表示単位を隠す。行が毎フレーム組み直される方式を既定にしない。

境界は以下を区別する:
- layout_bounds: レイアウト用の幅・高さ・行ボックス。
- ink_bounds: 実際の字形・線などが占める領域。
- visual_bounds: shadow/glow/blur/transformを含む描画領域。

背景帯の自動追従はlayout_bounds等の明示段階を参照する。
`背景幅 <- テキスト幅 + 余白` と `テキスト折り返し幅 <- 背景幅` のような循環は検出して拒否する。
解像度別の最大行数、最小文字サイズ、安全領域、overflowの方針をテンプレートの制約として持つ。

### 6.4 描画バックエンドの制約

確認したVelloのrender_to_texture APIはRgba8Unormを要求する。この経路をHDR全画面合成へ直結しない。
文字や単色形状のcoverageマスクを生成して、RGBA16F側で色を適用する構成を優先する。
任意のグラデーションや色付きSVGまで「coverageだけで完全再現できる」とは扱わず、必要な色処理は独自GPU描画パスで実装する。
同じベクターIRから異なるラスタライザーへ渡せるようにし、Velloを作品の保存形式にしない。

## 7. レンダラーとGPU

### 7.1 レンダー要求

```text
RenderRequest:
  snapshot_hash
  output_port
  instance_path
  sample_time
  spatial_region
  render_scale
  sampling_scope / shutter_policy
  quality_profile
  color_pipeline_id
  required_features
```

ノードは要求に応じて必要な入力時刻・領域を返す。出力ポートはColor/Maskを初期実装し、Depth/MotionVector/Normalは将来の型として境界を確保する。
値の評価とGPUコマンド発行を分離する。純粋モデル層はwgpu::TextureやAVFrameを保持しない。

OpenFXの入力領域/必要フレームの問い合わせに似た契約を参考にするが、OpenFXホスト互換をこの段階で約束しない。

### 7.2 合成

内部の色付き画像は、明示した作業用線形空間とpremultiplied alphaを基本とする。
Groupの既定は子を一度まとめてからGroup opacity/効果を適用するisolated方式。
各子にopacityを配る最適化は意味が一致する場合だけ行う。
Blend modeが表示基準の色空間を必要とする場合は明示変換を置き、すべて線形で同じ見た目になるとしない。
外部出力のstraight/premultiplied alpha変換、ゼロalpha付近、マット境界を検証する。

### 7.3 高解像度

RGBA16Fの3840x2160は63.28125 MiB、7680x4320は253.125 MiB（画像データのみ）。
デコード面、参照フレーム、中間テクスチャ、字形アトラス、蓄積バッファ、エンコーダーを別に予算化する。
macOSの共有メモリを独立したVRAMと同じ予算計算にしない。
CPU/GPU往復、GPU内コピー、FrameBridgeの同期待ちを計測する。
Vulkan/Metal/D3D12との相互運用は専用モジュールに隔離し、参照デバイス・所有権・同期の契約をテストする。

### 7.4 モーションブラー

一般の幾何アニメーションは複数サブフレームで評価できるようにする。
露光時間は `shutter_angle / 360 / output_fps` と定義し、シャッター位相も設定に含める。
基準実装は、各サブ時刻でComposition全体を合成してから重み付き平均する。各レイヤーを個別に平均してから重ねるだけでは重なりの意味が変わりうるため、無条件な置換をしない。
NLEのカット境界を跨ぐブラーは既定で避け、クリップ境界の方針を明示する。
ネストごとにサンプル数を掛け合わせないよう、sampling_scopeと共通時刻要求を共有する。
サンプルを順次蓄積して、全サンプル画像を同時保持しない。
動画内の被写体の真のサブフレーム像が復元されるわけではない。オプティカルフローは別モジュール。

### 7.5 キャッシュ

```text
cache_key = hash(
  node semantic version,
  dependency content hashes,
  instance input hashes,
  local sample time + time-map version,
  requested region + scale,
  quality + AA + shutter configuration,
  font/layout/vector versions,
  color pipeline,
  seed,
  simulation checkpoint identity when needed
)
```

Property値、Layout、Geometry、Raster、Effect frame、Simulation checkpointを別キャッシュにする。
構造変更でcompile、値変更で該当部分評価、色変更で組版再利用という粒度を目標にする。
厳密モードの画素キャッシュはデバイス・ドライバー・エンジンのfingerprintで分ける。
時間依存やSimulationの変更では将来方向への無効化を適切に広げる。

## 8. Repeater・Particles・音声連動・3Dの拡張点

### 8.1 Repeater

一つのSourceノードとinstance transforms/instance propertiesを保持する。
複製数分の編集オブジェクトを必ず生成する方式にしない。個別編集が必要な場合だけexpandを明示する。
描画のまとめ方はBlend/Mask/Effectで変わるため、常に単一draw callになると約束しない。
要素IDとinstance seedを固定し、配列の処理順が乱数に影響しないようにする。

### 8.2 Simulation

通常のアニメーションは `value = f(snapshot, time, instance)`。
状態を要する粒子等だけ `state(k+1) = step(state(k), inputs(k))`。
固定刻み、固定seed、版付き状態、入力ハッシュ、チェックポイントを使う。
シーク時は必要な時刻より前の有効なcheckpointから再計算する。逆再生は元の正方向シミュレーション時刻を参照し、数値積分を逆方向に巻き戻さない。
同じ環境内の結果一致を検証し、GPU原子演算等を含む完全なクロスデバイス決定性を別問題として扱う。
分散レンダーは未ベイクSimulationを含む区間を自由分割しない。

### 8.3 音声連動

RMS、帯域エネルギー、onset/beat等を事前解析し、タイムスタンプ付きDataAssetとして保存する。
Asset hash、サンプルレート、窓長、ホップ長、解析版、time-mapを固定する。
毎描画フレームで音声全体を再解析しない。ミックス後音声の特徴量を使う場合は対象Busとmix snapshotを固定する。
声の速度を自動変更せず、テンプレートSEはintro/outro markerへ配置する方式を優先する。

### 8.4 2.5D/3D

当初は2D。次に平面、奥行き、カメラの2.5DをScene3Dサブグラフで実装する。
2Dの描画順と3Dのdepth/transparency処理を同一のz値だけで統一しない。
将来のglTF、キャラクター等は明示した3D境界から取り込み、外部レンダラーの色・alpha・必要な補助チャンネルを合成できるようにする。
フル3D、リグ、物理、パストレーサーを動画編集MVPの必須条件にしない。

## 9. テンプレート

公開入力はText、Color、Number、Enum、MediaSlot、DataTable等の型とする。
公開していない内部Propertyをバッチ操作で勝手に書き換えない。上級編集の操作は別権限・別意図として扱う。
入力から内部Propertyへbindし、template definitionとinstance inputsは別保存。
テンプレート更新はバージョンを固定し、移行計画とプレビュー比較を生成する。既存作品を自動更新しない。

### 9.1 尺の伸縮例

Authoring duration 5秒、intro 0.4秒、outro 0.3秒とする。
8秒に変更すると、intro/outroは0.4/0.3秒を保持し、中間を7.3秒にする。
中間が静止ならhold、動く背景ならloopまたはstretchを明示指定する。
総尺が保護区間と最低hold時間の合計を下回ればDURATION_TOO_SHORTにする。

```json
{
  "template_id": "lower_third_ja",
  "version": "1.0.0",
  "public_inputs": {
    "headline": {"type":"text", "required":true},
    "subtitle": {"type":"text", "default":""},
    "accent": {"type":"color", "default":"#F59E0B"},
    "logo": {"type":"media_slot", "optional":true}
  },
  "duration_policy": {
    "intro": {"num":"2", "den":"5"},
    "outro": {"num":"3", "den":"10"},
    "minimum_hold": {"num":"1", "den":"2"},
    "middle_mode": "hold"
  },
  "layout_policy": {
    "max_lines": 2,
    "overflow": "error",
    "variants": ["landscape", "portrait"]
  }
}
```

上記は提案スキーマの例であり、稼働中製品の設定形式ではない。色入力には色空間の既定規約を適用し、保存時には明示色表現へ正規化する。

## 10. CLI / MCP / Agent

### 10.1 読み取りAPI

- capabilities.get: 対応ノード、補間、出力、色、GPU処理経路。
- scene.query: 範囲・タグ・種類・IDで検索。ページング。
- property.schema: 型、単位、アニメーション可否、参照可能段階。
- property.sample: 指定時刻列で値、source、modifier結果を返す。
- scene.explain: 親変換、マスク、opacity、時刻範囲、欠落資産などを診断。
- render.explain: 使用経路、CPU/GPU転送、中間メモリ、キャッシュ再利用を報告。
- preview.render: フレーム・短区間・コンタクトシートの成果物を生成。
- project.validate: 構造・文字・資産・機能・性能予算の診断を返す。

### 10.2 変更API

`composition.create`, `scene.node.add`, `scene.parent.set`, `animation.keyframes.upsert`, `expression.bind`, `template.instantiate`, `template.inputs.set`, `instance.retime`等の型付き操作をtransactionへ格納する。
GUIからも同じ操作を使う。

```text
inspect -> draft operations -> edit.plan -> preview(candidate snapshot)
        -> validate -> edit.apply -> render.submit -> job.get -> artifact.get
```

base_revision、idempotency_key、plan_hash、policyを明示する。
同一キーの異なるpayloadは拒否し、同一要求の再送は同一結果を返す。
計画後に文書が変わった場合は競合とし、勝手に古い計画を適用しない。
大きな計画のvalidation/compileはcommitの前に行うが、commit時に基準revisionを再確認する。

### 10.3 操作例（提案CLI）

```bash
ved template instantiate \
  --project demo.ved \
  --template lower_third_ja@1.0.0 \
  --inputs inputs.json \
  --duration 8s --plan-out plan.json

ved edit apply --project demo.ved --plan plan.json --json

ved preview render --project demo.ved \
  --composition comp_lower_third \
  --times 0s,0.2s,0.4s,4s,7.7s,7.9s \
  --quality final --out-dir ./preview --json

ved validate --project demo.ved --profile delivery --json

ved render --project demo.ved --profile hevc-4k \
  --out ./output.mp4 --wait --events ndjson
```

標準出力はJSON/NDJSON、標準エラーはログ。非対話モードでは質問せず、必要な権限・入力がなければ型付きエラー。
MCPはプロトコル対応版を交渉し、JSON SchemaとstructuredContentで構造化結果を返す。
長時間レンダーはアプリ内の永続ジョブにし、MCP接続の寿命に依存させない。

### 10.4 安全性

素材の文字列や字幕は命令ではなくデータ。通常操作にshell、任意FFmpeg引数、外部URL fetchを混在させない。
WASM拡張を導入する場合もWASI権限を原則与えず、fuel/epoch、メモリ、host callの制限を別々に設定する。
WASMのCPU命令制限は、そこから発行したGPU処理時間を制限するものではない。
未知のシェーダーやネイティブプラグインは別信頼区分にし、初期の自動化は組み込みノードに限定する。

## 11. Rustワークスペースと実装責務

初期は以下を論理モジュールとして開始し、ビルド依存やテスト境界に応じてcrate分割する。過度なmicro-crate化はしない。

```text
crates/
  ved-model/          # IDs, document types, property descriptors, versions
  ved-time/           # rational time, ranges, TimeMap, sampling
  ved-animation/      # curves, interpolation, modifier contracts
  ved-expr/           # typed AST, dependencies, bounded evaluator
  ved-scene/          # composition, parenting, masks, Scene IR
  ved-layout/         # responsive constraints, metrics, bounds
  ved-text/           # fonts, Japanese layout, glyph/cluster mappings
  ved-vector/         # paths, shape IR, geometry operations
  ved-render/         # DAG compiler, region/time planner, scheduler
  ved-gpu/            # wgpu, pipelines, color/alpha, texture pools
  ved-media/          # FFmpeg integration, seek, decode/encode
  ved-framebridge/    # OS/GPU specific interop and synchronization
  ved-audio/          # mixer, buses, feature-data integration
  ved-store/          # SQLite, snapshots, migrations, event journal
  ved-template/       # typed inputs, bindings, duration, versions
  ved-service/        # commands, queries, policies, job orchestration
  ved-cli/            # machine-oriented CLI adapter
  ved-mcp/            # MCP adapter
  ved-desktop/        # GUI adapter, timeline, canvas, graph editor
```

依存の向きはmodel/time -> animation/scene/text/vector -> render -> backend。
store/service/UIは評価エンジンを呼ぶが、評価エンジンはstoreやUIへ逆依存しない。
GPU texture、AVFrame、SQLite connection、Tokio runtimeの型をmodelに漏らさない。
FFmpegやwgpuのAPI差分はアダプターで吸収し、Cargo.lockとnative dependencies manifestを固定する。

## 12. 実装マイルストーン

| 段階 | 成果物 | 主な完了条件 |
|---|---|---|
| M0 | 基盤契約・テスト素材・技術スパイク | 有理数時刻/ID/Property/色/alpha規約、2D titleからGPU出力の最短経路 |
| M1 | Headless 2D Motion Core | Shape/Text/Group/Null、キーフレーム、任意時刻レンダー、画像連番 |
| M2 | NLE統合・CLI/MCP | CompositionClip、日本語title、固定snapshot、計画/適用、書き出し |
| M3 | 実用的なMotion Authoring | GUI canvas/curve editor、テンプレート、基本式、responsive layout |
| M4 | 高品質・高解像度 | サブフレームブラー、temporal cache、8K/HDR品質、GPU経路診断 |
| M5 | 高度な2D Motion | Repeater、path演出、音声連動、ルビ・縦書き、Simulation |
| M6 | 拡張 | 2.5D、外部レンダー、互換アダプター、プラグイン、分散 |

M0でGPU interopの困難さを確認するが、zero-copyの完全達成をM1のCPU検証版まで阻害する必須条件にはしない。
M1/M2は互換経路でも実装を進め、転送コストを明示する。GPU経路の保証はプラットフォーム/形式ごとに昇格する。

## 13. 最初の縦断テスト作品

10秒の4Kシーケンスに動画を配置し、5秒の日本語lower-thirdを重ねる。
角丸背景、二行の日本語、ロゴ、intro 0.4秒、outro 0.3秒、基本shadow、移動・opacityのキーフレームを含む。
同じ定義を8秒、別テキスト、縦型variantで再利用する。
CLIで生成し、MCPで値・layout boundsを検査し、GUIで同じ結果を閲覧し、固定スナップショットから書き出す。

受け入れ条件:
1. 文字数変更に背景帯が追従し、overflowは検出される。
2. 5→8秒でintro/outroの長さが変わらない。
3. 同じ時刻を順序を変えて要求しても同じ意味的結果になる。
4. 同じテンプレートの別インスタンスの入力が混ざらない。
5. GUI/CLI/MCPの操作は同じrevision/eventへ到達する。
6. 値とレイアウトの比較は厳密、GPU画素比較は固定環境の基準と許容誤差で行う。

## 14. 品質・性能の検証

### 正しさ
- 24, 25, 30, 30000/1001, 60000/1001fps、VFR、48kHz音声、長尺。
- ランダムアクセス、逆順、ネスト、loop、freeze、境界時刻。
- 結合濁点、IVS、絵文字、異体字、禁則、ルビ、縦書き。
- Group opacity、マット、blur halo、alpha edge、HDR->SDR表示と出力分離。
- 文字変更による範囲セレクターの再割り当て。
- template version update、未知機能の保持、migration失敗時の元データ保全。

### 障害
- GPU device lost、VRAM不足、ディスク不足、素材hash不一致、フォント不足。
- 途中切断、重複要求、古いrevision、取消、worker再起動。
- 循環式、巨大Path、過大複製数、長大テキスト、zip展開上限、外部参照拒否。

### 性能目標（未計測の受け入れ案）
- GPUを使わない単純な値/レイアウト更新がUI操作を長時間ブロックしないこと。
- 参照シーンを固定し、warm/cold、proxy/full、preview/finalを別々に測定する。
- 基本4K30プレビューで1フレーム33.3ms内を目標とし、デコード待ちとレンダーのみを分けてp50/p95を報告する。
- 60fpsは同じ品質での追加目標であり、8K、多重ブラー、全エフェクトについて一律保証しない。
- 参照機候補: RTX 4060 Ti 16GB/Windows、M4 Mac mini 32GB/macOS、Linux/NVIDIA runner。各環境は別結果を持つ。
- 8K/HDRはまず正しいoffline出力を合格条件にし、リアルタイム要件は別ベンチマークにする。

計測項目: compile_ms, eval_ms, layout_ms, raster_ms, gpu_ms, decode_wait_ms, encode_wait_ms, CPU/GPU transfer bytes, peak_memory, cache_hit_ratio, samples_per_frame。

## 15. 依存関係と優先順位

最優先はTIME、MODEL、PROP、SCENE、TEXT、RENDERの契約。
GUIの装飾、プラグイン数、完全な3D、クラウド分散はこの後。
実装タスクの依存関係・受け入れ条件は同梱の`motion_editor_backlog_v0_2.json`に記載する。これは未実装の計画データであり、GitHub等に登録したIssueではない。

## 16. 参考資料（一次資料、2026-10-01確認）

この仕様の独自設計部分は採用案であり、以下の仕様へ完全準拠するとの主張ではない。

- W3C Web Animations: stateless / hierarchical timing model。`https://www.w3.org/TR/web-animations-1/`
- Adobe After Effects Responsive Design – Time: protected regions。`https://helpx.adobe.com/after-effects/desktop/motion-graphics/add-responsive-design/responsive-design.html`
- OpenFX Image Effect Actions: RegionsOfInterest / FramesNeeded / sequential render。`https://openfx.readthedocs.io/en/main/Reference/ofxImageEffectActions.html`
- Unicode UAX #29: grapheme clustersとglyphの関係。`https://www.unicode.org/reports/tr29/`
- W3C JLReq: 日本語の禁則・ルビ・縦書き等。`https://www.w3.org/TR/jlreq/`
- Parley: text layout。`https://docs.rs/parley/latest/parley/`
- Vello Renderer: render_to_textureのフォーマット制約。`https://docs.rs/vello/latest/vello/struct.Renderer.html`
- wgpu Device: create_texture_from_halの安全条件。`https://docs.rs/wgpu/latest/wgpu/struct.Device.html`
- kurbo: 2D geometry。`https://docs.rs/kurbo/latest/kurbo/`
- lyon: path tessellation。`https://docs.rs/lyon/latest/lyon/`
- usvg: SVG parser/simplifier。`https://docs.rs/usvg/latest/usvg/`
- MCP Tools: JSON Schema / structuredContent。`https://modelcontextprotocol.io/specification/2026-07-28/server/tools`
- Wasmtime Config: consume_fuel / epoch_interruption。`https://docs.wasmtime.dev/api/wasmtime/struct.Config.html`
- OpenTimelineIO timeline structure: effect semanticsはapplication-specific。`https://opentimelineio.readthedocs.io/en/latest/tutorials/otio-timeline-structure.html`
