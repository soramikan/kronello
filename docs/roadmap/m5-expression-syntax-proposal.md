# OQ-17: M5 の式構文案（未採用）

状態: 利用者の判断待ち。ADR-0040 / 0058のAST正本・有界評価・禁止能力を変えない。以下はEXPR-002を実装するための具体案であり、承認前に採用済みと扱わない。

## 入力と保存

入力は式一つだけ。文・代入・変数定義・ループ・JavaScript互換・任意関数呼び出しを持たない。空白は意味を持たず、文字列はJSON形式の引用とescapeを使う。式のID・出力型・budget・意味版は編集対象のmetadataとして保持し、文字列の再解析で勝手に生成し直さない。

数値は有限の十進数（指数表記可）。符号付き数値literalを許し、非literalの負値は `0 - x` と記す。演算子は `+ - * /`、通常の乗除優先・左結合、括弧で明示できる。演算の順序を変える最適化をparser/formatterで行わない。

## 記法

| 意味 | 表示例 |
|---|---|
| 時刻（通常TimeMap適用後のauthored source scopeの秒） | `time()` |
| 算術 | `10 + sin(time()) * 5` |
| 既存関数 | `clamp(x, 0, 1)`、`lerp(a, b, t)`、`sin(x)` |
| 型付き構築 | `vec2(x, y)`、`vec3(x, y, z)`、`angle(degrees)` |
| 固定hash noise | `noise(seed, element, input)`（seed/elementはu32 literal） |
| 同じscopeのProperty | `property("node-uuid", "property-uuid", "scalar")`。Composition inputはnodeを`null`にする |
| Curve sample | `curve("curve-uuid", time_offset("1", "24"), "scalar")`。offsetは正規化有理数で保存する |
| 任意の既存Value literal | `literal("<Value JSONをJSON文字列としてescapeしたもの>")`。Scalar/String/Bool等の簡便literalと併用し、型情報を落とさない |

Propertyは表示名で解決せず、GUIの参照挿入で安定IDと型を入れる。動的な任意文字列を参照先として評価しない。Vec2等の定数Value literalと、子式を持つVec2構築nodeを区別し、必要なら`literal(...)`で元のAST構造を保持する。

M5作業ブランチで受け入れたAUDIO-001 / EXPR-003のASTを、次の関数で表す案とする。これは構文の未採用案であり、parserの実装済み仕様ではない。

| ASTの意味 | 表示例・引数 |
|---|---|
| `PropertySample` | `property_sample("node-uuid", "property-uuid", "scalar", lookback)`。先頭3引数は静的参照、最後だけ式。Composition inputのnodeは`null` |
| `DataAssetCell` | `data_cell("asset-uuid", "column", row, "scalar")`。asset・column・型はliteral、rowだけ式 |
| `ContinuousNoise` | `continuous_noise(seed, element, input)`。seed/elementはu32 literal、inputは式 |
| `AudioFeature::Rms` / `Onset` / `Beat` | `audio_feature("asset-uuid", "rms", time_offset("0", "1"))`。featureは`"rms"` / `"onset"` / `"beat"`のいずれか |
| `AudioFeature::BandEnergy` | `audio_band("asset-uuid", band, time_offset("0", "1"))`。bandはu32 literal |

`lookback`は共有評価器と同じ非負秒数で、root timeline上で1nsへ量子化してから配置時間へ写像する。`row`は非負整数の範囲検証を共有評価器へ渡す。構文上の浮動小数点演算を保存時刻の正本へ置き換えない。`time_offset`は単独の式nodeではなく、対応する関数の固定有理数引数だけに許す構文とする。分子・分母はi64範囲の十進文字列として読み、共有Time型で正規化する。ゼロ分母や正規化時のoverflowは拒否する。

型名は公開`ValueType`のsnake_case値に限定する。Boolは`true` / `false`、StringはJSON文字列を簡便literalとし、EnumとString、Colorの色空間、AssetRef等を区別する必要がある場合は`literal(...)`を用いる。未知関数・未知型・動的な参照IDを拒否する。構文は既存ASTの表示であり、任意I/Oや再帰評価を解禁しない。

## 往復と診断

- この構文上限で表現可能な正規ASTに対し `parse(format(ast), metadata) == ast` を検証する。metadataは元ExpressionのID / version / value_type / budgetで、本文からは復元せず明示編集境界から渡す。formatter出力が64KiB / 8192 tokens / depth64を超える既存ASTは型付き診断を返し、元ASTを保持する。既存ASTすべてがこの構文上限に収まるとは保証しない。
- 入力文字列の空白や括弧配置をそのまま保存する保証はしない。`format(parse(text))`を正規表記とする。
- byte範囲・行・列・期待tokenを持つ構文診断をGUIとAPIで共有する。日本語入力中の未確定文字列は作品へ適用しない。
- 型・単位・依存・循環・予算の検証は既存AST評価器を使う。入力長・token数・入れ子深さにも有限上限を設け、構文不正時は既存値を保持する。
- 共有edit.plan/apply、revision競合・Undo・固定snapshotを維持する。

## 構文処理の上限案

入力はUTF-8で64KiB、token数8192、構文の入れ子64までとする。上限を超えた入力はAST構築前または構築中に診断を返す。さらに既存Expressionのnodes / dependencies / memory / instructions / samples予算を適用し、構文の上限が評価予算を拡張しないようにする。上限値を含めてOQ-17の採用判断対象とし、承認後のADRと回帰試験へ固定する。

formatterはASTの左右の部分木・postorder順・literalの型を保持する。例えば`a - (b - c)`を`a - b - c`へ変換せず、定数畳み込みや型付きVec2 literalとVec2構築nodeの置換もしない。元ASTが対応する意味版の範囲内であることを先に確認し、parse/formatによって意味版やbudgetを暗黙に上げない。

## 往復表現の補足（未採用案）

`seed` / `element` / `band` のu32 literalは十進数字のみで0〜4294967295、符号・小数点・指数表記を許さない。これはScalarの有限十進数literalとは別の固定引数であり、u32をf64経由で保存しない。`row` / `lookback` はASTのu32子node indexを直接書く引数ではなくScalar子式で、parserがpostorder indexを構築する。rowの整数性/範囲とlookbackの非負秒数は既存評価器が検証する。

Scalar簡便literalはf64へ変換し、formatterは元の有限f64を復元できる表記を出す。`-0`を含む符号も保持し、literalを演算nodeへ変換しない。`literal(...)` のJSONは共有Valueのserde表現そのものとし、未知field/typeや非有限値を拒否する。構築関数 `vec2(...)` 等は常に構築node、Value::Vec2等の定数は常に `literal(...)` とする。これにより値が同じでも異なるASTを混同しない。

関数のarityは表の通り固定し、`time()`は0引数、`sin` / `angle`は1、`vec2`は2、`vec3` / `clamp` / `lerp` / `noise` / `continuous_noise`は3、`property` / `curve` / `audio_feature` / `audio_band`は3、`property_sample` / `data_cell`は4、`literal`はJSON文字列1引数。metadata、固定参照、固定有理数引数を子式として評価しない。
