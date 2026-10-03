# ADR-0044: 作業用線形色と alpha の入出力契約を固定する

- 状態: 採用
- 日付: 2026-10-03

## 背景

ADR-0024 は作業用色空間とタグなし sRGB 入力、ADR-0037 は HDR 方針を決定した。05 章には内部 premultiplied alpha があるが、Color Property と画像の区別、外部形式の alpha 表現、ゼロ付近の変換が検証項目にとどまっていた。ARC-001 / GPU-001 の規約として具体化する。

本 ADR は ADR-0024 / ADR-0037 を置換・書き換えせず、決定済みの色規約を確認して alpha と変換境界を追加する。HDR の演算・実機検証を完了したとは扱わない。

## 決定

### 既存決定の確認: 作業用色空間と入力

| 対象 | 維持する規約 |
|---|---|
| SDR | 新規 Sequence の既定は線形 Rec.709（sRGB 原色、D65） |
| HDR / 広色域 | 当該出力プロファイルを選んだ Sequence の既定は線形 Rec.2020（D65） |
| Composition | 固有の作業用色空間を持たず、配置先 Sequence に従う。単体要求は色パイプラインを明示し、省略時は線形 Rec.709 |
| 保存色 | `{space, components}` の明示表現。色空間を持たない色は保存しない |
| タグなし色入力 | `#F59E0B`、0〜255 の RGB 等は非線形符号化された sRGB と解釈し、保存時にタグ付き表現へ正規化する |
| 評価・補間 | 保存色を作業用線形空間へ変換して評価し、Color 補間も既定で同じ空間を使う |
| 中間画像・キャッシュ | RGBA16F。RGB の負値・1 超を出力変換まで保持。作業用色空間と色処理の版を `color_pipeline_id` / キャッシュキーに含める |
| HDR 方針 | 基準白 203 cd/m²、Rec.2100 の PQ / HLG。SDR の白を基準白へ配置。表示・明示 SDR 出力に限ってトーンマッピングと色域圧縮を行う |

M0〜M3 は SDR のみで、HDR 実装は COLOR-001（M4）。GPU-001 では線形 Rec.2020 への作業色変換も検証するが、HDR の輝度・伝達特性・出力対応の保証とは区別する。Rec.2020 を規約に含めることと、HDR 対応を `capabilities.get` で宣言できることを区別する。未対応の HDR 最終出力は [ADR-0045](0045-snapshot-compatibility-boundaries.md) に従って拒否する。

### 今回固定する規約: 色の値と表現

- 保存する Color Property / 公開 Color 入力の RGB は straight（alpha を掛けない）で、alpha は独立した `[0, 1]` の有限値とする。色空間タグと `components` 内の RGB / alpha の具体的スキーマは PROP-001 で定義する。
- 8bit RGB 入力は各成分を 255 で正規化して sRGB として保存する。入力 alpha の省略時は 1。sRGB の通常の入力成分は `[0, 1]` とし、範囲外の作業値は線形色空間を明示して渡す。
- 線形 Rec.709 と非線形 sRGB は原色・白色点を共有するが、同じ数値表現ではない。sRGB の伝達関数を復号してから作業空間に変換する。単なるタグの付け替えや一律の gamma 2.2 で代用しない。
- alpha / coverage は無次元の被覆・不透明度であり、sRGB / PQ / HLG の伝達関数を適用しない。
- HDR 作業値の 1 は基準白（203 cd/m²）に対応させる。1 超は許す。SDR 作業値の 1 は SDR の白であり、すべての表示器で一定の輝度を意味するものではない。
- Color の既定補間は、作業空間に変換した straight RGB と alpha をそれぞれ補間する。画像の premultiplied 表現でのフィルタリング・合成とは区別する。別の補間を提供するときは意味の版と補間モードを明示する。

### 今回固定する規約: 内部画像

- 内部 Color 画像は作業用線形空間の premultiplied alpha とする。straight RGB を `C`、alpha を `a` とすると、内部値は `(C * a, a)`。
- `a = 0` の内部 RGB は `(0, 0, 0)` に正規化する。Property に保存した透明色の straight RGB まで消さない。
- `0 < a <= 1` の RGB に `0 <= RGB <= a` を課さない。HDR・広色域変換で生じる負値や 1 超を保持する。非有限値はエラーとする。
- 通常の source-over は `RGB_out = RGB_src + RGB_dst * (1 - a_src)`、`a_out = a_src + a_dst * (1 - a_src)`。opacity / coverage は premultiplied RGB と alpha の両方に同じ倍率で掛ける。
- 画像の補間、blur、モーションブラーの蓄積は premultiplied 値で行う。Group は既存の isolated 規約に従い、子をまとめた結果に Group opacity を適用する。
- Mask 出力は色空間を持つ RGB ではなく、有限の coverage `[0, 1]`。Mask を色変換したり、RGB の明度と暗黙に取り違えたりしない。

### 今回固定する規約: 外部境界とゼロ付近

- デコード、ベクターの色付き raster、外部レンダー、エンコード、プレビュー面の各アダプターは、色空間・伝達特性と alpha の `straight / premultiplied / opaque` を契約に明示する。premultiplied の場合は関連付けた RGB が線形か符号化済みかも明示する。
- 形式の規定・metadata・明示入力設定から alpha の表現を確定できない場合、画素から推測しない。設定不足を型付きエラーで報告する。alpha チャンネルがないと確定した入力は opaque（alpha = 1）。タグなし「色入力」の sRGB 規約を、色情報不明の動画素材すべてに拡張しない。
- 非線形色変換は straight RGB に行う。外部 premultiplied 入力は、入力側の関連付け空間で unpremultiply し、伝達関数・原色変換を行い、作業用線形空間で premultiply する。linear-premultiplied のまま非線形伝達関数を RGB に直接適用しない。
- unpremultiply の閾値を `alpha_epsilon = 2^-16` と固定する。`a > alpha_epsilon` なら `RGB / a`、`0 <= a <= alpha_epsilon` なら straight RGB をゼロにする。alpha 自体は保持する。後者は不安定な除算を避けるため RGB の復元を捨てる境界処理であり、可逆変換とは扱わない。
- この閾値は外部入出力で unpremultiply が必要な境界だけに適用する。内部 premultiplied 画像や straight Property の小さい alpha を丸めたり、通常合成で捨てたりしない。内部 effect 等が straight RGB を必要とする場合は、`a = 0` だけ RGB をゼロとし、正の alpha では十分な精度で除算する。内部の範囲外 RGB を出力変換前に消さない。閾値変更は色処理の意味の変更であり、版と `color_pipeline_id` を変える。
- straight 外部出力では上記の規約で復元した RGB に出力変換を行い、独立 alpha として渡す。premultiplied 外部出力では、形式が指定する関連付け空間で再度 alpha を掛ける。alpha を持たない出力は出力プロファイルに明示した背景へ合成してから渡し、alpha を黙って捨てない。

## 影響

### レビュー結果と修正

| 照合した文書 | 確認結果・不足 | 今回の対応 |
|---|---|---|
| ADR-0024、05 / 07 章 | 線形 Rec.709 / Rec.2020、sRGB 入力の規約は一致 | 既存決定として確認し、伝達関数と straight Color の境界を補足 |
| ADR-0037、05 章 | HDR 方針と M4 の実装時期は一致。作業値 1 の意味が未記載 | HDR の 1 を基準白へ対応させ、規約と対応機能の宣言を分離 |
| 03 / 05 章 | Color 補間の空間はあるが alpha 表現が未記載。画像の premultiplied と混同しうる | straight Color 補間と premultiplied 画像処理を区別 |
| 05 / 06 章 | 外部 alpha とゼロ付近は検証項目のみ | 表現の明示、変換順、閾値と非可逆性を固定し、外部 3D 境界にも参照を追加 |
| ADR-0012、04 / 05 章 | coverage raster と RGBA16F 合成の分離は一致 | coverage に色の伝達関数を適用しないことを明記 |

### 後続タスクの検証契約

- GPU-001: タグなし sRGB と同じタグ付き色の一致、線形 Rec.709 への復号、線形 Rec.2020 作業空間でも共通の比較空間へ戻すと同じ色を意味すること、色変換を経た premultiply / unpremultiply、透明な有色入力、`a = 0`、閾値の直下・一致・直上、alpha = 1、source-over、Group opacity、マット境界を固定環境で検証する。閾値以下では RGB の完全な round-trip を要求しない。
- PROP-001: straight Color の補間と alpha の独立補間、sRGB の保存時正規化を検証する。色空間の省略を保存モデルへ持ち込まない。
- COLOR-001: Rec.2020、203 cd/m²、PQ / HLG、表示と HDR / SDR 出力の分離を検証する。トーンマッピング・色域圧縮の演算詳細は引き続き COLOR-001 の設計対象。
- 代替案は、Color Property も premultiplied で保存すること、ゼロ以外を常に除算すること、外部 alpha を推測すること。透明色の編集値を維持し、ゼロ付近の不安定性とアダプターによる解釈差を避ける上記の規約を選ぶ。

## 関連

- [ADR-0024](0024-working-color-space.md)、[ADR-0037](0037-hdr-policy.md)、[ADR-0012](0012-hdr-compositing-vs-vector-rasterizer.md)
- [ADR-0043](0043-semantic-dependencies-and-units.md)、[ADR-0045](0045-snapshot-compatibility-boundaries.md)
- [03 プロパティ](../architecture/03-property-animation.md)、[04 ベクター・テキスト](../architecture/04-vector-text-layout.md)、[05 レンダラーと GPU](../architecture/05-render-gpu.md)、[06 拡張点](../architecture/06-extensions.md)、[07 テンプレート](../architecture/07-templates.md)
