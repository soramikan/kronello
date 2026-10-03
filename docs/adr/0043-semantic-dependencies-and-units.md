# ADR-0043: 編集モデル・評価・レンダーの依存境界と単位を固定する

- 状態: 採用
- 日付: 2026-10-03

## 背景

ARC-001 は TIME-001 / PROP-001 / GPU-001 が共有する意味の契約を固定する。Timeline と Composition の分離（ADR-0002）、純粋評価（ADR-0003）、排他的な PropertySource（ADR-0004）、意味的スナップショットと資源の分離（ADR-0005）は決定済みだが、処理の流れとコード依存の区別、単位・座標系が不足していた。

本 ADR は既存 ADR を置換しない。以下の「既存決定の確認」は継承し、「今回固定する規約」を追加する。契約の採用であり、実装・実機検証が済んだことを意味しない。

## 決定

### 既存決定の確認

- Timeline は Sequence / Clip の placement model、Composition は scene model とし、共通の Scene IR / Render DAG へコンパイルする。
- Composition を Timeline の SourceRef として配置でき、Composition のネストは循環禁止。Timeline の配置・トリム・リップルの意味を Composition に持ち込まない。
- Property は Constant / Curve / Expression の一つと順序付き Modifier から成り、通常評価は `(snapshot, time, instance)` の純粋関数。状態を要する Simulation は別境界とする。
- 時刻は正規化された有理数、区間は `[start, end)`、JSON の分子・分母は 10 進文字列。編集グリッドと連続評価時刻を区別する。
- 設計寸法と出力画素数を分け、同じ縦横比で解像度だけを変えても原則再レイアウトしない。

### 今回固定する規約: 意味の参照と評価順序

`Timeline → Composition → Property → Render` は、配置からシーン、値の評価、描画へ至る意味の追跡順である。全 Clip が Composition を経由するわけではなく、Asset / Generator も共通 IR に入る。これは crate の一方向の import 列を表さない。

| 境界 | 読むもの・生成するもの | 禁止する逆参照 |
|---|---|---|
| Timeline | SourceRef、時間写像、配置区間を読み、配置先の instance / local time を確定する | Composition の定義や Property 評価から配置元の編集状態を探索しない |
| Composition | ノード、所有・変換親、入力束縛を読み、Scene IR を構築する | Timeline のトラック・リップルを解釈しない |
| Property | 明示した upstream Property / DataAsset / Layout の依存を読み、型付き値を返す | store / service / UI、GPU 資源、評価履歴を読まない |
| Render | Scene IR、評価値、時刻・領域・色の要求を読み、Render DAG と実行計画を作る | 描画結果で文書や Property の主値源を書き換えない |

Property / Layout は依存 DAG の順に評価する。組版に必要な Property と、確定した bounds を読む下流 Property を区別する。`背景幅 ← テキスト幅 + 余白` と `折り返し幅 ← 背景幅` の循環は拒否し、描画済み画像を読み返して解決しない。

### 今回固定する規約: 論理モジュールとコード依存

以下の `A → B` は「A が B の型・契約に依存する」。[11 ワークスペース](../architecture/11-workspace.md) の論理モジュールに対応し、独立 crate 化を義務づけるものではない。

| 論理モジュール | 依存先と責務 |
|---|---|
| `kronello-time` | 有理数時刻・区間・TimeMap。編集モデル・評価・バックエンドに依存しない |
| `kronello-model → kronello-time` | ID、Timeline / Composition の文書型、Property descriptor、版、スナップショット。Curve / Expression は ID や意味的データで参照し、評価実装に依存しない |
| `kronello-animation / kronello-expr → kronello-model / kronello-time` | 曲線・Modifier・AST・依存列挙・有界評価。scene / render に逆依存しない |
| `kronello-text / kronello-vector → kronello-model / kronello-time` | 組版・幾何の意味的結果。GPU 描画や Timeline の編集操作に依存しない |
| `kronello-layout → kronello-model / kronello-time / kronello-text` | 制約・計測・bounds。必要な評価値を引数で受け取り、scene / render への逆依存を作らない |
| `kronello-scene → kronello-model / kronello-time / kronello-animation / kronello-expr / kronello-layout / kronello-text / kronello-vector` | Composition、親子関係、マスク、Scene IR の構築。Timeline の文書型は model から受け取る |
| `kronello-template → kronello-model / kronello-time` | 型付き入力・束縛・尺・版の契約。必要なコンパイルは上位から scene に渡す |
| `kronello-render → kronello-model / kronello-time / kronello-scene` | Timeline 配置と Scene IR を共通 DAG にまとめ、時間・領域・資源予算を計画する。必要な評価・組版・幾何のモジュールも利用できる |
| backend (`kronello-gpu / kronello-media / kronello-audio`) | model / time と render が定義するバックエンド非依存の入出力契約を利用できる。具象 GPU / FFmpeg / 音声資源はここに閉じる |
| `kronello-framebridge` | gpu / media の具象アダプター間の OS・GPU 相互運用と同期。純粋層から参照しない |
| `kronello-store → kronello-model / kronello-time` | SQLite、直列化、migration。評価エンジンは store に依存しない |
| `kronello-service` | store、template、scene、render、backend、framebridge を組み合わせる。`kronello-cli / kronello-mcp / kronello-ffi → kronello-service` |

render は具象 backend を import せず、自身の契約を満たす実装を上位の service / worker から受け取る。コールバックや trait による実行時呼び出しは、具象 crate への依存と区別する。backend 同士の相互運用は framebridge に隔離する。

純粋層の公開型に `wgpu::Texture`、`AVFrame`、SQLite connection、Tokio runtime を含めない。Property が Layout 結果を読む場合も、依存の入力として意味的値を渡し、animation / expr から layout / scene への循環 import を作らない。

### 今回固定する規約: 単位と座標

以下の単位名は意味の契約であり、具体的な Rust 型・JSON の enum 表記は PROP-001 で定義する。Property の `units` と座標空間を明示し、同じ Scalar / Vec2 という理由で異なる単位を混用しない。

| 対象 | 正本の規約 |
|---|---|
| 時刻・duration | 秒を単位とする有理数 `num / den`。`den > 0`、最大公約数は 1、ゼロは `0/1`。負時刻は表現できるが、duration は非負。ゼロ分母と checked 演算の overflow は型付きエラー |
| 2D 長さ | 設計座標の単位 `design_px`。design_extent、位置、anchor、Path、線幅、フォントサイズ、余白、bounds に使う。画面の DPI、GUI の point、出力画素とは別 |
| 2D 座標 | Composition の設計矩形左上が `(0, 0)`、+X は右、+Y は下。ノードの内容はローカル座標を持ち、anchor と変換で親空間へ写す。負座標・矩形外の値を許す |
| 出力領域 | 出力の左上を原点とする画素単位。画素 `(i, j)` は境界 `[i, i+1) × [j, j+1)`、中心は `(i+0.5, j+0.5)`。設計座標からの写像を RenderRequest の領域・scale と区別する |
| 2D 角度 | 度。rotation / skew / shutter_angle に適用。+rotation は +X を +Y に回す画面上の時計回り。角度を 360 度で剰余化せず、0→720 度を 2 回転として保持する。三角関数の内部計算でだけ radian に変換する |
| scale | 無次元の倍率。1 が等倍、負値は反転。ゼロ倍率の表現は許すが、逆変換が必要な処理では特異変換を診断する |
| opacity / alpha / coverage | 無次元の有限値 `[0, 1]`。percent 表示は入口で変換する。alpha / coverage は色の伝達関数を適用しない |
| Color | 色空間を持つ成分と独立した alpha。非線形 sRGB の通常入力 RGB は `[0, 1]`（8bit 入力は 255 で正規化）。線形作業 RGB は有限の負値・1 超を許す。詳細は [ADR-0044](0044-color-and-alpha-contracts.md) |

値に NaN / infinity を保存・評価結果として通さない。opacity / alpha / coverage の範囲違反は検証エラーとし、暗黙の clamp を行わない。Property の範囲検証は主値源と順序付き Modifier を適用した最終値に対して行う。overshoot を許す曲線から範囲内の結果を得るには、明示 Modifier を使う。線幅・フォントサイズなど個別 Property の有効範囲は descriptor に宣言する。

外部形式・GUI の単位や座標系は共有 API の境界で正本に変換する。3D の軸・角度型や OQ-17 の式言語構文はここで決めない。

## 影響

### レビュー結果と修正

| 照合した文書 | 確認結果・不足 | 今回の対応 |
|---|---|---|
| ADR-0002〜0005、00 / 01 / 06 章 | モデル分離・純粋評価・資源分離は一致。参照順とコード依存の区別が不足 | 上記の境界表を追加し、各章から参照 |
| 11 章 | 旧図の `model / time → … → render → backend` は構築順か import 方向か不明で、model が評価・backend に依存するとも読める | 処理の流れと `利用側 → 提供側` のコード依存を分け、逆依存禁止を明記 |
| 02 / 03 / 04 章、用語集 | 有理数・連続角・設計寸法の分離は一致。秒、正規化条件、座標原点・軸、単位・範囲が未記載 | 単位表を固定し、各章の該当規約と用語を補足 |
| 03 / 04 章 | Property / Layout の循環禁止はあるが、crate 循環を避ける受け渡しが未記載 | 意味的な入力値と依存 DAG による受け渡しを明記 |

### 後続タスクの検証契約

- TIME-001: `2/4 → 1/2`、`1/-2 → -1/2`、`0/n → 0/1`、ゼロ分母、overflow、負時刻、`[start, end)` の端点を検証する。非線形 TimeMap の量子化・丸め・版の詳細は TIME-001 で設計し、浮動小数点時刻を保存する逃げ道にしない。
- PROP-001: 単位と有効範囲を descriptor に持ち、0→720 度、非有限値・opacity 範囲違反の拒否、明示 Modifier を検証する。
- GPU-001: +90 度が +X を +Y へ写すこと、設計寸法を変えずに出力解像度だけ変えられることを意味的値と画素で照合する。色・alpha の契約は ADR-0044 を併用する。
- 代替案として、出力画素と設計寸法の統一、角度の radian 保存、render から具象 backend への直接依存を検討した。既存の解像度分離・度の例・純粋層の分離を保ち、CPU 評価とバックエンド差し替えを可能にする上記の契約を選ぶ。

## 関連

- [ADR-0002](0002-timeline-composition-separate-models.md)、[ADR-0003](0003-pure-evaluation-at-arbitrary-time.md)、[ADR-0004](0004-exclusive-property-source.md)、[ADR-0005](0005-semantic-snapshot-vs-gpu-resources.md)
- [ADR-0044](0044-color-and-alpha-contracts.md)、[ADR-0045](0045-snapshot-compatibility-boundaries.md)
- [00 概要](../architecture/00-overview.md)、[01 データモデル](../architecture/01-data-model.md)、[02 時間](../architecture/02-time.md)、[03 プロパティ](../architecture/03-property-animation.md)
- [04 ベクター・テキスト](../architecture/04-vector-text-layout.md)、[06 拡張点](../architecture/06-extensions.md)、[11 ワークスペース](../architecture/11-workspace.md)、[用語集](../glossary.md)
