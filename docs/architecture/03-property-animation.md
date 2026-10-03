# 03 プロパティとアニメーション

## Property

```text
PropertySource<T> = Constant(T) | Curve(CurveId) | Expression(ExpressionId)
Property<T> = Source + ordered Modifiers<T>
```

主値源は Constant / Curve / Expression のいずれか一つであり、複数の値源が暗黙に上書きし合うことはない（[ADR-0004](../adr/0004-exclusive-property-source.md)）。

位置、回転、スケール、不透明度、色、線幅、マスク、エフェクト、公開入力は同じ Property 基盤に乗せる。

### 単位と範囲

[ADR-0043](../adr/0043-semantic-dependencies-and-units.md)「単位と座標」に従い、Property の descriptor に単位・座標空間・有効範囲を宣言する。

- 位置・anchor・Path・線幅・フォントサイズ・余白・bounds は設計単位 `design_px`。Composition の左上原点、+X は右、+Y は下とし、ノード内容はローカル座標で保持する。
- rotation / skew は度、scale は無次元倍率（1 が等倍）。+rotation は画面上の時計回りで、連続角を剰余化しない。
- opacity / alpha / coverage は有限の `[0, 1]`。非有限値・範囲違反はエラーにし、暗黙に clamp しない。範囲を制限する Modifier は明示する。
- 保存 Color は色空間タグ付き straight RGB と独立 alpha。線形 RGB は負値・1 超を許す（[ADR-0044](../adr/0044-color-and-alpha-contracts.md)）。

組版に必要な Property と、確定した Layout bounds を読む下流 Property は依存 DAG で分ける。意味的な結果を入力として渡し、animation / expr から layout / scene / render への循環 import を作らない。

## 変換

編集時の変換は 2D で次のとおり定義する。

```text
T(position) * R(rotation) * K(skew) * S(scale) * T(-anchor)
```

親子付け変更では、ローカル変換を維持するか、画面上の見た目を維持するかを操作で指定する。

## 補間規約

| 型 | 補間規約 |
|---|---|
| Scalar / Vec2 / Vec3 | Hold / Linear / Cubic |
| Angle | 巻き戻さない連続角。0→720 度の 2 回転を保持 |
| Color | 補間色空間を明示。既定は作業用線形空間の straight RGB と alpha の独立補間（ADR-0044）。画像の premultiplied フィルタリングとは区別 |
| Bool / Enum / String / AssetRef | 離散切り替えのみ |
| Path | 点数・セグメント型・対応が一致する場合のみ morph |
| Transform3D | 将来の独立型。Quaternion 等の意味を版管理 |

時間方向のイージングと空間的な移動パスを分ける。ベジェ曲線の時間ハンドルは時刻方向の単調性を検証する。
キーフレームの同時刻重複は暗黙に許容せず、upsert / replace を操作として選ばせる。

### ANIM-001 の実装規約

文書型 `AnimationCurve` / `Keyframe` / `CurveInterpolation` / `TimeBezier` は `kronello-model`、純粋評価 `sample` / `sample_in_space` は `kronello-animation` に実装する。既存の `CurveId`、`Value`、`ValueType`、`kronello-time::Time` を再利用し、Property / Modifier 全体の評価は EVAL-001 以降で扱う。

- 曲線は `id`、`value_type`、厳密な時刻順の `keys`、意味の版 `interpolation_version` を保存する。版 1 のみ編集・評価する。未知の版は既知構造として読み取り・再保存できるが、最新の意味に置換しない。未知フィールド・補間 variant などを保持できない構造は拒否し、呼び出し側が原本を保全する。
- キーの時刻は秒の正規化有理数。`insert_key` は同時刻を拒否し、`upsert_key` は挿入または置換、`replace_key` は既存キーだけを置換する。失敗時は曲線を変更しない。構築・復元時も重複や順序違反を拒否し、暗黙に並べ替えない。
- 各キーの補間方式は次のキーまでの `[key.time, next.time)` に適用する。キー時刻そのものは当該キー値、最初より前・最後より後は端の値を保持する。空曲線の評価は型付きエラー、1 キーは全時刻で保持する。Color は端点・Hold も作業空間へ変換して返す。
- Cubic は正規化した `(0, 0)` から `(1, 1)` への時間・値進行のベジェとする。有限の制御点で `0 <= x1 <= x2 <= 1` を要求し、時刻方向の単調性を保証する。y は有限の overshoot を許す。空間移動パスの接線ではない。版 1 は有理数で区間内の比を求めてから f64 に変換し、64 回の二分法で x を反転して y を評価する。表現不能な有理数演算・非有限の評価結果は型付きエラー。
- Scalar / Vec2 / Vec3 / Angle は各成分を補間し、角度を剰余化しない。Bool / Enum / String / AssetRef と現段階の Path は Hold のみ。Path morph は VEC-002 で扱う。
- Color は sRGB の伝達関数を復号し、必要なら既存 GPU 参照と同じ D65 原色変換係数を f64 で適用する。`sample` は単体要求の線形 Rec.709、`sample_in_space` は明示した線形 Rec.709 / Rec.2020 を使い、straight RGB と alpha を独立に補間する。Sequence の作業空間や descriptor の明示空間は呼び出し側から渡す。負値・1 超の線形 RGB は保持し、alpha の `[0, 1]` 違反は clamp せずエラーとする。Rec.2020 の値変換は HDR 出力対応の宣言ではない。
- descriptor の範囲は Modifier 列の適用後に既存の `validate_final_value` で検証する。曲線評価は範囲 clamp や暗黙の代替値を行わない。数値 golden、境界、編集の原子性、および 128 個の生成曲線の順方向・逆順・固定 seed の順序変更と JSON 往復を通常テストで確認する。

### EVAL-001 の実装規約

純粋 crate `kronello-eval` の `EvaluationSnapshot` は Composition / Curve / descriptor と評価側の依存宣言を借用する。`DependencyGraph::compile` は配置ごとにグラフを構築し、`evaluate_property` / `evaluate_properties` / `evaluate_scene` は呼び出し内だけのメモ化で任意の有理数時刻を評価する。保存・版・資産 lock の互換性検証は、この型付き入力を生成する呼び出し側の責務であり、保存層への依存や最新 Project の暗黙参照は持たない。

- ノードの実行時キーは既存 `PropertyKey`（`InstancePath`, `NodeId`, `PropertyId`）を使う。所有 Node のない Composition 入力は `RuntimePropertyKey::Composition`（`InstancePath`, `CompositionId`, `PropertyId`）として区別し、仮の NodeId は生成しない。
- 保存された `PropertySource` は Constant / Curve / Expression のまま保つ。評価側だけの `ReferenceBindings` は既存の配置入力束縛を親スコープの Property 参照で上書きし、型・単位・座標空間を照合する。`DependencyDeclarations` は一般の静的依存辺を表し、将来 EXPR-001 が AST から生成する接続点とする。AST 自体を実行する機能ではない。循環は閉じた全実行時キー経路を持つ `DependencyCycle` として報告する。
- 定義の Curve は配置の `local_time_map` を親から順に適用したローカル時刻で評価する。配置に記された入力束縛の Curve は、その束縛を記した親 Composition の時刻で評価する。`sample_in_space` に作業用線形色空間（descriptor の指定があればそれ）を渡し、値源・参照束縛の結果を `validate_final_value` で検証する。Expression、有効な未実装 Modifier、未知の補間版は `UNSUPPORTED_FEATURE` とする。無効な Modifier は実行しない。
- `EvaluatedScene` は containment の `root_nodes` / `child_order` による前順序で配置を展開する。Group / Null / CompositionInstance の枠と containment 親、ローカル opacity、各配置の入力値を保持し、後続 RENDER-001 が隔離合成を判断できる。配置の内部ルートは配置枠の直後、配置自身の所有する子より先に並ぶ。`active_range` は各スコープのローカル時刻に対する `[start, end)` で判定し、非アクティブな所有親の子・配置内容は展開しない。
- 変換は既存 builtin descriptor の既定値を使い、`T(position)*R(rotation)*K(skew)*S(scale)*T(-anchor)` を計算する。`K(skew)` は度で指定する X shear（`x' = x + tan(skew)*y`, `y' = y`）とする。world 変換は `transform_parent` の鎖と外側の配置変換を継承し、containment や描画順から導出しない。非アクティブな transform 親も必要な変換値を供給する。計算で非有限の行列が生じた場合は型付きエラーとする。
- 通常テストで 257 個の有理数時刻、Property 要求順、ノード保存順の順方向・逆順・固定 seed のシャッフルを比較する。入れ子の時刻変換、参照束縛、自己循環・複数 Property / 配置間の循環、半開区間、未対応機能、最終値の型・範囲、変換行列を検証する。

## 式

最初は型付き AST と許可された組み込み関数で実装する。式の正本は常に AST である。

人間向けには、中置演算と関数呼び出しだけの小さな式言語を後から追加する（[ADR-0040](../adr/0040-expression-language-policy.md)）。文・ループ・代入は持たず、AST と一対一に往復でき、JavaScript 互換にはしない。構文の詳細は未決（[OQ-17](../open-questions.md)）。

対象とする機能:

- 演算、clamp、lerp、周期関数
- 固定 seed の noise
- upstream Property 参照
- 版固定 DataAsset 参照

与えないもの（[ADR-0009](../adr/0009-sandboxed-expression-evaluation.md)）:

- ネットワーク、ファイル、現在時刻、環境変数
- 非固定乱数、無制限ループ、動的ノード探索

式の参照先・サンプル時刻数・ノード数・命令数に予算を設ける。

### 乱数と決定性

`random(seed, instance_id, element_id)` は評価呼び出し順に依存させない。
時間変化する noise は時刻を明示引数とする。
異なる GPU / CPU 間の浮動小数点まで無条件にビット一致するとは約束しない。

### 自己参照と失敗

通常式からの再帰的な自己参照は禁止する。以前の値を積み上げる表現は Simulation へ移す。
失敗時に最終出力で勝手に基底値へ置換しない。プレビューの代替表示は警告付きにし、最終出力はエラーにする。
