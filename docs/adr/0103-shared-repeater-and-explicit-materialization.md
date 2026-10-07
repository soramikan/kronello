# ADR-0103: 共有 Source の Repeater と明示 materialization

- 状態: 採用
- 日付: 2026-10-06
- 関連: REPEAT-001、ADR-0003、ADR-0007、ADR-0045、ADR-0102

## 文脈

同じ図形・文字・Composition を複数配置しても、通常操作で Source 全体を個数分の編集オブジェクトへ複製しない。各配置の識別と乱数座標は表示名や配列位置から独立させ、個別の Source 編集を必要とする操作だけを明示する。TemplateInstance を含む Source の個別化で、公開入力、variant、文字帯、時間写像を失ってはならない。

## 決定

`Project.repeaters` に版付き `DocumentObject<Repeater>` を保存し、`NodeKind::Repeater { content_ref }` が参照する。版 1 の Source は `RepeatSource { composition, root }`。共有 Composition の単一 root を明示する。root は Group を含む通常ノードでよいが、containment / transform parent を持たない。複数 root の既存 Composition や外部の transform parent を持つ部分木を自動コピー・切り離ししない。利用者は通常の Node 編集で明示した Group root を構成するか、`REPEATER_SOURCE` を受け取る。

各 `RepeatInstance` は保存済みの `CompositionInstanceId`、配置 `NodeId`、`u64` seed、enabled、`[start,end)` active_range、rational local_time_map、Property / Effect、Composition input bindings を保持する。instance 配列は描画順だけを表す。通常のコンパイルでは Repeater を Group と共有 Source を参照する CompositionInstance 配置へ純粋に lowering する。Source の編集オブジェクトを個数分保存しない。既存の effect / matte / blend isolation と時間写像を利用し、単一 draw call を保証しない。

Noise の instance 座標へ保存 seed を明示的に加える。Repeater context がない既存式の UUID 座標は変更しない。並べ替えや異なる呼出し順は同じ ID の値に影響しない。

GUI / CLI / MCP は既存の共有 `edit.plan` / `edit.apply` を使う。

- `RepeaterSet`: Source と instance 設定を作成・変更する。
- `RepeaterRemove`: Repeater record を削除する。参照を残す変更は全体検証で拒否する。
- 通常の `PropertySourceSet` / Modifier / Curve 編集は instance の保存済み placement ID を指定できる。
- `RepeaterExpand { repeater, instance, expansion_id }`: 指定 instance だけを個別の Source に materialize する。

expand は original Source / TemplateDefinition / TemplateInstance を変更せず、必要な Composition、図形、文字、Curve、Expression、matte relation をコピーする。新しい所有 ID は、明示した fresh expansion UUID と元の UUID に SHA-256 を適用した UUIDv8。配列番号・表示名・時刻は使わない。instance 自身の ID / placement / seed は維持する。`expanded_source` の参照が個別 Source を選択し、他の instance は共有 Source を使い続ける。コピーした nested instance の `noise_aliases` は元の Noise 座標を維持する。文字列・表示名・asset / font 参照は所有 ID の置換対象ではない。

nested TemplateInstance は選択 variant と公開入力を解決してから、配置ごとのコピーへ反映する。Property の source、文字内容と uniform style の範囲、Media slot を個別編集可能な通常モデルへ materialize し、placement の時間写像を保持する。DataTable の投影も既存の `input_bindings` を使う。動的な band bounds / max-lines 規則は `ExpandedRepeatSource.layout_constraints` にコピーして、同じ TemplateRuntime の layout scheduler で実行する。元 template の入力 override metadata を捨てて画面を変える方式や、特定時刻の描画だけをベイクする方式にしない。既存 Template の未対応入力（非 uniform style の文字入力等）は同じ型付きエラーで拒否する。

保存と undo は通常の project / event transaction を使う。後続の個別編集と衝突する expansion undo は既存の `UNDO_CONFLICT` を維持する。後続編集を黙って削除しない。

## 互換性と境界

`RenderSnapshot.semantic_versions.repeater = 1` を新規 snapshot に固定する。旧 snapshot の pin 不在を許すのは authored Repeater record がない場合だけ。必要な未知版・opaque record は `UNSUPPORTED_FEATURE`。missing Source、単一 root 不一致、重複 identity、cycle、materialized layout の不整合を `REPEATER_MISSING` / `REPEATER_SOURCE` / `REPEATER_IDENTITY` / `REPEATER_CYCLE` / `REPEATER_LAYOUT` で拒否する。1 record の instance 上限は 1024、Composition lowering / materialization は既存の 1024 definition と評価 scope budget に従う。

Source が同じでも instance properties、effects、blend、matte、時間写像が違えば実行結果や pass 数は異なる。Source sharing は編集モデルと定義共有の契約であり、GPU batching の約束ではない。

## 検証

受け入れ証拠と実 CLI / MCP / Metal 手順は [REPEAT-001 検証記録](../testing/repeat-001.md)。設計採用はタスク完了や main 統合を意味しない。
