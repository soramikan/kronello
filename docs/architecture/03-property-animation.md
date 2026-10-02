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
