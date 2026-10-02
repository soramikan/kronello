# 03 プロパティとアニメーション

## Property

```text
PropertySource<T> = Constant(T) | Curve(CurveId) | Expression(ExpressionId)
Property<T> = Source + ordered Modifiers<T>
```

主値源は Constant / Curve / Expression のいずれか一つであり、複数の値源が暗黙に上書きし合うことはない（[ADR-0004](../adr/0004-exclusive-property-source.md)）。

位置、回転、スケール、不透明度、色、線幅、マスク、エフェクト、公開入力は同じ Property 基盤に乗せる。

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
| Color | 補間色空間を明示。既定は作業用線形空間 |
| Bool / Enum / String / AssetRef | 離散切り替えのみ |
| Path | 点数・セグメント型・対応が一致する場合のみ morph |
| Transform3D | 将来の独立型。Quaternion 等の意味を版管理 |

時間方向のイージングと空間的な移動パスを分ける。ベジェ曲線の時間ハンドルは時刻方向の単調性を検証する。
キーフレームの同時刻重複は暗黙に許容せず、upsert / replace を操作として選ばせる。

## 式

最初は型付き AST と許可された組み込み関数で実装する。人間向け DSL は後から同じ AST へ変換する。

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
