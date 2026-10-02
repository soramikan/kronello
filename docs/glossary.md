# 用語集

型名・API 名は英語のまま用いる。

## 編集モデル

| 用語 | 意味 |
|---|---|
| Project | 作品全体。資産、Sequence、Composition、Template を持つ。1 プロジェクト = 1 つの `.koma` ファイル |
| Timeline | カット編集（NLE）の編集モデル。クリップ配置、トリム、リップル、リンクを扱う |
| Sequence | Timeline の具体的な入れ物。尺、フレームレート、音声レート、作業色空間、トラックを持つ |
| Clip | トラック上の配置単位。SourceRef と時間範囲、TimeMap を持つ |
| SourceRef | Clip が参照する元。Asset、Composition、Generator を区別する |
| Composition | モーショングラフィックスの編集モデル。空間階層を持つシーン |
| SceneNode | Composition 内のノード。初期種類は Group、Null、Shape、Text、Media、CompositionInstance |
| CompositionInstance | 別の Composition を参照して配置するノード。入力束縛とローカル時間写像を持つ |
| InstancePath | 親からたどった CompositionInstance の安定 ID 列。同じ定義の複数配置を区別する |
| containment parent | 所有・描画順上の親 |
| transform parent | 変換を継承する親。containment parent とは独立 |

## 値と時間

| 用語 | 意味 |
|---|---|
| Property | 型・単位を持つアニメーション可能な値。主値源と Modifier 列から成る |
| PropertySource | Property の主値源。`Constant` / `Curve` / `Expression` のいずれか一つ |
| Modifier | 主値源の結果に順に適用する変換 |
| AnimationCurve | キーフレームと補間の集合 |
| TimeMap | 親の時刻からローカル時刻への写像（線形、区分線形、将来は逆再生・ループ等） |
| edit rate | 編集用フレームグリッド。評価時刻とは別で、フレーム間でも評価できる |
| posterize / hold | 内部レートを保持して評価時刻を量子化する明示的な指定 |

## 資産

| 用語 | 意味 |
|---|---|
| Asset | 不変の素材。content hash と locator を持つ |
| DataAsset | 解析結果などの不変データ（ASR、音声特徴量、マスク等）。解析版と time mapping を固定する |
| RenderSnapshot | レンダーの入力を固定したもの。revision、資産・フォント・データの lock、意味の版を含む |

## テキストとレイアウト

| 用語 | 意味 |
|---|---|
| 書記素クラスタ | Unicode UAX #29 の利用者が知覚する 1 文字。グリフと一対一ではない |
| AnimationUnit | 文字アニメーションの単位。組版クラスタを壊さない |
| LayoutResult | 組版結果（行、グリフ、bounds、アンカー） |
| layout_bounds | レイアウト用の幅・高さ・行ボックス |
| ink_bounds | 字形や線が実際に占める領域 |
| visual_bounds | shadow / glow / blur / transform を含む描画領域 |
| design_extent | Composition の設計上の寸法。出力画素数とは別 |
| responsive variant | 縦横比ごとの明示的なレイアウト定義 |

## レンダー

| 用語 | 意味 |
|---|---|
| Scene IR | Timeline / Composition をコンパイルした意味的な中間表現。幾何と組版結果を保持する |
| Render DAG | 色・マスク・画像の処理順序を表す型付きグラフ |
| output port | ノードの出力種別。初期は Color / Mask。Depth / MotionVector / Normal は将来 |
| RenderRequest | 時刻・領域・品質などを指定するレンダー要求 |
| sampling_scope | モーションブラー等の時間サンプルを共有する範囲 |
| isolated group | 子を一度まとめて合成してから opacity や効果を適用する Group |
| FrameBridge | デコーダー / エンコーダーと wgpu の間でフレームを受け渡す OS・GPU 依存のモジュール |
| Simulation | 状態を持つ評価（粒子等）。固定刻みと checkpoint を使う専用ノード |

## テンプレート

| 用語 | 意味 |
|---|---|
| TemplateDefinition | 版付きのテンプレート定義。Composition、公開入力、尺の方針、制約を持つ |
| public input | テンプレートが外部へ公開する型付き入力（Text、Color、MediaSlot 等） |
| instance inputs | テンプレートインスタンスごとの入力値。定義とは別に保存する |
| duration policy | intro / outro の保護区間、最小 hold、中間区間の扱い |

## API と保存

| 用語 | 意味 |
|---|---|
| Command / Query API | 全入口（GUI / CLI / MCP）が共有する変更・読み取り API |
| Project Service | 変更を直列化し、revision・policy・監査を扱うサービス |
| revision | プロジェクトの版番号。変更トランザクションごとに進む |
| plan | 適用前の変更計画。`base_revision` と `plan_hash` を持つ |
| idempotency key | 同じ要求の再送を同一結果にするためのキー |
| `schema_version` | 保存構造の版 |
| `semantic_version` | 補間・合成などの意味の版 |
| job | 長時間処理（レンダー等）。接続の寿命から独立した永続的な単位 |
