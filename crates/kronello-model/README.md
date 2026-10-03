# kronello-model: PROP-001

型付き Property と schema registry の純粋モデル層。保存・検証する型を実装し、曲線・式・Modifier の評価、GPU 資源、ストレージは含まない。

## 公開 API

- `PropertyId` / `CurveId` / `ExpressionId` / `DescriptorId` / `AssetId` / `ModifierId`: 独立した UUID 型。
- `SchemaKey`: 表示名と独立した名前空間付きの安定キー。小文字 ASCII・数字・`_`・`-` の非空セグメントを `.` で区切る。
- `FiniteF64` / `ValueType` / `Value`: Scalar、Vec2、Vec3、Angle、Color、Bool、Enum、String、AssetRef、Path。全数値成分を有限値に限定する。
- `Color` / `ColorSpace` / `ColorComponents`: 色空間タグ付き straight RGB と独立 alpha。`from_srgb8` / `from_srgb_hex` は保存表現へ正規化する入力アダプター。
- `Unit` / `CoordinateSpace` / `InterpolationMode` / `ValueRange`: 型と整合する単位・座標・補間能力、成分ごとの開閉区間。
- `DescriptorDefinition` → `PropertyDescriptor`: 入力定義を検証し、ID・キー・version・意味の metadata を固定する。表示名だけ変更可能。
- `SchemaRegistry`: 検証済み descriptor を安定キーで登録・照会する。重複キー・ID を拒否し、キーの辞書順で列挙する。`with_builtin()` は固定 UUID v4 の標準 descriptor 8 件を登録して返す。
- `PropertySource<T>` / `Property` / `DescriptorRef` / `Modifier`: 排他的な主値源、版固定 descriptor 参照、順序付き Modifier の記述枠。
- `SourceResolver`: Curve / Expression の存在・戻り値型を意味的 metadata だけで照合するインターフェース。
- `ModelError` / `JsonError`: 検証エラーと JSON 構造の互換性エラーを分ける。

## 実装上の決定

### ID とキー

UUID v4 を採用した。中央採番なしで複数の編集プロセスが ID を生成でき、配列順・表示名・ローカライズに依存しない。編集時に一度生成して保存し、評価時に生成しない。UUID の型を分け、Property / Curve / Expression 間の取り違えを防ぐ。descriptor の UUID は個別定義の identity、`SchemaKey` は schema 作者が与える照会キーであり、いずれも表示名から導出しない。

### JSON の値源

`PropertySource<T>` は隣接タグ方式を採用した。`kind` が唯一の値源を選び、`value` に定数または UUID を一つだけ置く。内部タグ方式ではスカラー値や UUID を同じ形で表現できないため、payload を別フィールドにした。

```json
{"kind":"constant","value":{"kind":"angle","value":720.0}}
```

```json
{"kind":"curve","value":"f5916c19-d0ec-451d-8c1f-034758bd70b0"}
```

`Constant` / `Curve` / `Expression` は Rust enum でも排他的。source の変更は enum 全体の置換であり、旧値源を残さない。追加 payload・重複タグ・未知タグは拒否する。`set_source` / `set_modifiers` は検証成功後に変更し、失敗時は元の Property を保持する。

### 未知フィールドと版

本 crate の既知型では `deny_unknown_fields` を使う。未知フィールド・未知 enum 値を lossless に保持する文書外枠はまだ実装していないため、読めた部分だけを成功として返さず、JSON 境界で `JsonError::IncompatibleStructure` を返す。これは ADR-0045 の「lossless な保持を保証できない構造は原本を変更せず型付き互換性エラーで拒否する」境界に当たる。呼び出し側は原文を保持し、失敗した部分モデルを保存・export に使わない。

`PropertyDescriptor::from_json` は定義の構造読取後に検証するため、対応しない descriptor version や範囲違反などは `JsonError::Validation(ModelError)` で識別できる。汎用 `from_json<T>` と通常の serde 読取でも拒否するが、serde 内で生じる domain エラーは構造エラーに包まれる。

descriptor の構造・契約版は `version: 1` のみ対応し、それ以外は拒否する。参照には安定キーとその版を記録し、registry の最新版で補わない。registry は一つのキーにつき一つの版を保持し、登録済み metadata を置換できない。

Project / RenderSnapshot 全体の公開 JSON Schema 生成、`schema_version`・`semantic_versions`・migration、opaque な未知ノードの保持・再 export は STORE-001 等の文書保存境界に残す。本実装はこれらの完成や未知データの round-trip を主張しない。

`serde_json` の `float_roundtrip` を有効にし、有限 f64 の JSON 往復で意味的な値が変わらないようにする。極値、subnormal、負のゼロ、固定 seed のビット列サンプルで検証する。

### 単位・範囲・補間

`Unit` は `dimensionless` / `design_px` / `degrees`。Angle は度だけ、Path は設計単位だけを許す。設計座標の Vec / Path は `local_design`（ノード内容）/ `parent_design`（変換親）/ `composition_design`（Composition 全体）を明示する。Vec3 の保持自体は 3D の軸・変換・描画対応を意味しない。時刻・duration は `kronello-time` の有理数を使い、f64 の seconds 単位は設けない。

標準 descriptor のキーと既定値は次のとおり。すべて Hold / Linear / Cubic の補間能力を持つ。

| SchemaKey | 型・単位・座標空間 | 既定値・範囲 |
|---|---|---|
| `kronello.transform.position` | Vec2 / design_px / parent_design | `[0, 0]`・制限なし |
| `kronello.transform.anchor` | Vec2 / design_px / local_design | `[0, 0]`・制限なし |
| `kronello.transform.scale` | Vec2 / dimensionless | `[1, 1]`・負値とゼロを許可 |
| `kronello.transform.rotation` | Angle / degrees | `0`・制限なし、剰余化なし |
| `kronello.transform.skew` | Angle / degrees | `0`・制限なし、剰余化なし |
| `kronello.opacity` | Scalar / dimensionless | `1`・`[0, 1]` |
| `kronello.fill_color` | Color / dimensionless | 不透明な sRGB 黒 |
| `kronello.stroke_width` | Scalar / design_px | `0`・`[0, ∞)`（有限値のみ） |

固定 ID は `TRANSFORM_POSITION_ID` などの公開 `DescriptorId` 定数であり、登録時に生成しない。組み込みキー・ID と独自 descriptor の衝突も既存の重複検証で拒否する。

Scalar / Vec2 / Vec3 / Angle / Color の型能力は Hold / Linear / Cubic。Bool / Enum / String / AssetRef は Hold のみ。Path は現段階では Hold のみとし、対応する点数・セグメントを確認する morph は後続の animation / vector 層に残す。descriptor は能力の部分集合を宣言し、不正な補間モードを拒否する。ここでは補間の計算を実装しない。

数値範囲は min / max、省略可能な端点、inclusive / exclusive を表す。Vec の範囲は成分ごとであり、Scalar 範囲を暗黙に broadcast しない。opacity descriptor には有限の `[0, 1]` を宣言する。範囲違反を clamp しない。

ADR-0043 に従い範囲は Modifier 適用後の最終値で検証する。enabled Modifier がない Constant だけは構築時に最終範囲を検証する。Modifier がある場合も元の型は検証するが、overshoot を範囲だけで拒否せず、後続評価層が `validate_final_value` を呼ぶ。Modifier は順序・安定 ID・種類キー・版・enabled・型付き parameters の記述枠であり、clamp を含むアルゴリズムは未実装。未知の Modifier の実行可否は animation compiler が判定し、黙って評価を省略しない。

`Property::from_json(input, registry)` は descriptor 参照まで照合する。通常の serde 読取は registry を受け取れないため、intrinsic な構造検証のみ行う。import として受け入れる前に `validate(registry)` を必ず呼ぶ。曲線・式の存在と戻り値型は `validate_sources(registry, resolver)`、評価後の値は `validate_final_value` で確認する。

### Color

保存表現は次の形。alpha 省略入力は 1 と解釈するが、再保存時は alpha を必ず明示する。色空間の省略や hex 文字列を保存 Color として読み込むことは拒否する。

```json
{
  "space": "srgb",
  "components": {"r": 0.5, "g": 0.0, "b": 1.0, "alpha": 0.0}
}
```

sRGB RGB は `[0, 1]`、線形 Rec.709 / Rec.2020 RGB は有限の負値・1 超を保持する。alpha は全色空間で有限の `[0, 1]`。透明色も straight RGB を保持し、alpha を RGB に掛けたり伝達関数を適用したりしない。

8bit / hex のタグなし入力は各成分を 255 で除して `srgb` として保存する。これは入力正規化であり、線形化ではない。sRGB ⇄ 線形の伝達関数・原色変換・premultiply / unpremultiply は GPU-001 の色処理に残す。

Color descriptor の `color_interpolation_space: null`（Rust の `None`）は配置先 Sequence の線形作業空間を選択する既定値とし、明示する場合は `linear_rec709` / `linear_rec2020` を許す。Color 以外は `None` だけを許す。straight RGB と alpha の独立補間を意味の契約として記述するが、実際の Linear / Cubic 補間は M1 の `kronello-animation`、配置先 Sequence の作業空間に応じた変換は GPU-001 に残す。モデル側のテストは metadata と独立 alpha の保持を検証し、未実装の補間評価や HDR 出力対応を検証済みとは扱わない。

## 検証

```sh
cargo fmt -p kronello-model --check
cargo clippy -p kronello-model --all-targets -- -D warnings
cargo test -p kronello-model
```

`tests/property_schema.rs` は受け入れ条件と ADR-0043 / 0044 / 0045 のモデル境界を検証する。`PropertySource` の compile-fail doctest は異なる Constant 型と同時に複数の値源を構築する操作がコンパイルできないことを確認する。
`tests/builtin_registry.rs` は固定 UUID・キー、再構築と列挙の決定性、標準 descriptor の既定値・契約、範囲違反、連続角の保存、重複登録と衝突の拒否を検証する。
