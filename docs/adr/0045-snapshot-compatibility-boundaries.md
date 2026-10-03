# ADR-0045: 保存・意味・実行能力の互換性を分けて判定する

- 状態: 採用
- 日付: 2026-10-03

## 背景

ADR-0010 は未知機能の保持と最終出力の拒否、ADR-0029 は公開 JSON スキーマと未知フィールドの保持を決定した。01 / 09 章では版を区別しているが、RenderSnapshot の `semantic_versions` と Project の `semantic_version` の関係、未知フィールドを保持できることと実行できることの違いが不足していた。

本 ADR は既存の決定を置換せず、ARC-001 と後続の保存・モデル・レンダー実装の互換性境界を固定する。版番号の初期値や migration の実装が完成したとは扱わない。

## 決定

### 既存決定の確認

- 正本は SQLite の `.kronello`。JSON は不変スナップショットまたはインポート形式であり、第二の正本ではない。
- 文書モデルの公開 JSON スキーマを一つ定義し、ジョブ、export / import、Command / Query、FFI は共通の型定義を利用する（ADR-0029 / ADR-0031）。要求・応答の envelope がプロジェクト全体と同じ形であることは要求しない。
- `schema_version` は構造、`semantic_version` は補間・合成などの意味。未知フィールド・未知機能は保存で失わない。migration 失敗時は元データを壊さない。
- 必要な未知・未対応機能がある最終レンダーは `UNSUPPORTED_FEATURE`。式の失敗・資産欠落にも黙った代替を使わず、プレビューの代替表示には警告を付ける（ADR-0010）。

### 今回固定する規約: 版の境界

| 識別子 | 変える契機・用途 | 同一視しないもの |
|---|---|---|
| `schema_version` | 公開文書・スナップショットの構造と符号化の互換性。変更には版管理と必要な migration | SQLite 内部テーブルの版、アプリのリリース版、編集 revision |
| `semantic_version` | 文書全体の意味の契約。単位、座標、補間・合成の既定など、同じ文書値から得られる意味が変わる変更 | 保存構造が読めること |
| `semantic_versions` | RenderSnapshot に固定する意味の版の集合。文書の `semantic_version` と、利用する補間・TimeMap・組版・vector・色パイプライン・effect / Simulation 等の版を対応付ける | 単一のエンジン実行ファイルの版 |
| effect / template の version | 選択した実装・定義を固定。更新は明示的な移行・比較を経る | 自動的に最新を選ぶ指定 |
| `revision` | 編集トランザクションごとの変更順・競合検出 | スキーマ互換性や意味の互換性 |
| engine version / fingerprint | worker の実行ファイル、再現環境、厳密な画素キャッシュの区別 | 文書の意味を上書きする根拠 |

構造を変えず意味だけを変える場合も意味の版を変える。構造と意味の両方が変わる場合は両方を扱う。単純な版番号の大小や、未知フィールドを読み飛ばせたことだけで互換と判定せず、対応する構造・意味の版と migration の経路を明示する。版番号の表記・初期値、SQLite の内部版、公開 JSON Schema の具体的な定義は PROP-001 / STORE-001 で設計する。

### 今回固定する規約: 読み取り・保持・変更・実行

| 状態 | 許すこと | 拒否すること |
|---|---|---|
| 対応構造・対応意味 | 通常の検証を通した読み取り・変更・レンダー | 実行環境に不足する機能を黙って使うこと |
| 対応する外枠内の未知フィールド・ノード・enum / effect の値 | opaque データとして意味的に保持し、再保存・export で失わない。診断と、安全に独立していると証明できる既知部分の変更 | 未知部分の意味を推測して書き換え・既知の既定値へ置換すること |
| 解釈できない構造版、または lossless な保持を保証できない構造 | 元データを変更せず、型付き互換性エラーで報告。可能な範囲の読み取り専用診断 | 部分的に読んだ内容で元ファイルを上書きすること |
| 解釈できない意味の版・必要機能 | 内容の保持と診断。変更の安全性が証明できない場合は読み取り専用 | 最終レンダー、未知の意味に依存した変更 |

未知データの保持は JSON の空白・キー順の byte 一致ではなく、値・型・所属・参照を失わないことを意味する。import が要求する構造を安全に扱えない場合は拒否し、元の入力と既存プロジェクトを保持する。未知内容を保持できない export を成功扱いにしない。

### 今回固定する規約: RenderSnapshot

- 固定する入力は、文書の意味的内容、公開 `schema_version`、文書の `semantic_version` を含む `semantic_versions`、元の revision、資産・フォント・DataAsset の lock、出力・品質・色の profile とする。GPU 資源や SQLite connection を含めない。
- Project の `semantic_version` と snapshot 内の対応項目は一致させる。欠けた版を実行側の「最新」で補わない。互換性を検証済みの旧形式に対してだけ、明示 migration で必要項目を補える。
- 文書・版・lock・profile などレンダー結果を変える固定入力の変更は snapshot の content identity とキャッシュの区別に反映する。未知の意味的内容も無視しない。hash の正規化・直列化方式は STORE-001 で設計する。
- worker は開始・再開時に構造版、意味の版、必要機能、lock を照合する。同じエンジン版から起動したことだけで照合を省略しない。固定入力を最新の Project に置換しない。
- migration は原本を保全して検証し、成功した更新を原子的に確定する。過去の固定ジョブ入力は上書きせず、変更した入力からの実行は新しい snapshot / job として区別する。旧イベントを新しいコマンド意味で無条件に再実行しない。

### 今回固定する規約: 未知機能と最終レンダー

- `capabilities.get` は実装済みの機能・対応する意味の版・実行経路を報告し、`project.validate` と実際の render compile / worker の検証は同じ互換性判定を使う。
- 必要機能は選択した output port / instance / 時間・領域の要求と依存グラフから導出し、要求の `required_features` と合わせて検証する。呼び出し側が一覧を省略しても検証を回避できない。
- 出力に必要な未知ノード、補間、式、effect、色パイプライン、semantic version は `UNSUPPORTED_FEATURE` で最終レンダーを拒否する。必要性を判定できない opaque 内容は安全側に拒否する。未選択の Composition 等が依存しないと証明できる場合は、その未知機能だけを理由に当該要求を拒否しない。
- エラー診断は不足機能・要求版・対象 ID / InstancePath・要求出力を示す。構造の不正、式評価失敗、`ASSET_MISSING` / `ASSET_HASH_MISMATCH` はそれぞれの型付きエラーとして扱い、すべてを `UNSUPPORTED_FEATURE` にまとめない。
- プレビューの代替表示は警告と欠落箇所を示し、正常な最終品質の結果・キャッシュと区別する。`quality_profile` を final にするだけでプレビューの代替を正当化しない。最終出力は検証成功後だけ確定成果物へ切り替える。

## 影響

### レビュー結果と修正

| 照合した文書 | 確認結果・不足 | 今回の対応 |
|---|---|---|
| ADR-0010 / ADR-0029、01 / 09 章 | 未知内容を保持する規約は一致。ただし読み取れることと実行できることの区別が不足 | 互換性の状態表を追加し、安全に保持できない版は原本を保全して拒否 |
| 01 章 | Project に `semantic_version`、RenderSnapshot に `semantic_versions` があり、対応関係と snapshot の構造版が未記載 | snapshot に構造版を明記し、意味の版集合へ文書の版を含める |
| 05 / 08 / 14 章 | `required_features`、機能問い合わせ、worker のエンジン版はあるが実行前・再開時の判定が不足 | 依存からの必要機能導出と固定版・lock の照合へリンク |
| 09 章 | migration の原本保全はあるが、構造と意味の更新、既存固定入力への影響が未記載 | 明示 migration と固定 snapshot の非上書きを補足 |

### 後続タスクの検証契約

- PROP-001 / STORE-001: 未知フィールド・ノード・enum 値の read / export / import round-trip、未知構造版の拒否、migration 失敗時の原本保全、文書と snapshot の意味の版一致を検証する。
- GPU-001 / RENDER-001: 必要な未知 effect / 色処理が `UNSUPPORTED_FEATURE` になること、依存しない未知機能との区別、警告付きプレビューと最終出力の区別を検証する。
- JOB-001 / RECOVERY-001: 開始・再開で意味の版と lock を照合し、編集後も元の固定入力を使うことを検証する。完了区間の再利用範囲は RECOVERY-001 で設計する。
- 代替案は、schema 互換なら実行すること、未知フィールドを捨てること、worker が常に最新の意味へ更新すること。保存可能性と実行可能性を分け、原本と成果物を保全する契約を選ぶ。
- OQ-02 / OQ-14 / OQ-17 は解決せず、そのまま残す。

## 関連

- [ADR-0010](0010-unsupported-features-fail-final-render.md)、[ADR-0029](0029-public-json-schema.md)、[ADR-0005](0005-semantic-snapshot-vs-gpu-resources.md)、[ADR-0025](0025-detached-render-workers.md)、[ADR-0031](0031-ffi-c-abi-json.md)
- [ADR-0043](0043-semantic-dependencies-and-units.md)、[ADR-0044](0044-color-and-alpha-contracts.md)
- [01 データモデル](../architecture/01-data-model.md)、[05 レンダラーと GPU](../architecture/05-render-gpu.md)、[08 API・CLI・MCP](../architecture/08-api-cli-mcp.md)、[09 保存と同時編集](../architecture/09-storage-concurrency.md)、[14 ジョブ](../architecture/14-jobs.md)、[未決事項](../open-questions.md)
