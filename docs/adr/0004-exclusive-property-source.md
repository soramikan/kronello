# ADR-0004: Property の主値源は Constant / Curve / Expression のいずれか一つ

- 状態: 採用（v0.2 仕様から継承。実装による検証は未了）
- 日付: 2026-10-01

## 背景

キーフレームと式と定数が同時に存在し暗黙の優先順位で上書きし合うと、値の出どころが利用者にもエージェントにも分からなくなる。

## 決定

- `PropertySource<T> = Constant(T) | Curve(CurveId) | Expression(ExpressionId)` とし、主値源は排他的に一つ。
- 追加の変換は順序付きの Modifier として明示する。

## 影響

- `property.sample` が source と modifier の結果を一意に説明できる。
- 値源の切り替えは明示的な操作になる。

## 関連

- [03 プロパティとアニメーション](../architecture/03-property-animation.md)
