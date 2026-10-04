# 01 データモデル

## 最小オブジェクト

| オブジェクト | 主なフィールド |
|---|---|
| Project | schema_version, semantic_version, assets, sequences, compositions, templates |
| Asset | id, content_hash, kind, stream_metadata, immutable_locator |
| DataAsset | id, schema, content_hash, values, time_mapping, analyzer_version |
| Sequence | id, extent, frame_rate, audio_rate, working_space, tracks |
| Track | id, kind（video / audio）, clips |
| Clip | id, source_ref, timeline_range, source_in, time_map, volume, links, effects |
| Composition | id, duration, design_extent, edit_rate, root_nodes, properties, inputs, markers, output_ports |
| SceneNode | id, kind, containment_parent, transform_parent, child_order, active_range, transform_ref, content_ref |
| CompositionInstance | id, definition_ref, input_bindings, local_time_map, seed |
| Property | id, type, units, source, modifiers, validation, capabilities |
| AnimationCurve | id, value_type, keys, interpolation_version |
| TemplateDefinition | id, template_id, version, composition_ref, public_inputs, duration_policy, constraints, content_hash |
| TemplateInstance | id, definition_ref, version, duration, inputs |
| RenderSnapshot | content_hash, schema_version, revision, asset/font/data locks, semantic_versions, profile |

STORE-001 では `Project` の最小保存外枠として UUID `id`、`name`、構造版・意味版、Composition / Curve 集合、未知フィールドを実装した。TEMPLATE-001 は省略可能な `templates` / `template_instances` を追加し、定義の不変な版と配置ごとの入力を別保存する。MEDIA-001 の `assets` と NLE-001 の `sequences` も省略可能な集合として実装した。未知内容の保持と編集可否、公開 JSON Schema は [09 保存と同時編集](09-storage-concurrency.md) と [ADR-0046](../adr/0046-store-format-and-location-policy.md) を参照する。

SourceRef は Asset、Composition、Generator を区別する。SourceRef の型が増えても Clip の編集意味は変えない。

Timeline の文書型（Sequence / Clip）と Composition / Property descriptor は `kronello-model` に置き、評価実装は分離する。意味の参照と論理モジュールの依存境界は [ADR-0043](../adr/0043-semantic-dependencies-and-units.md) を参照。

NLE-001 は `Sequence`、video / audio `Track`、`Clip`、タグ付き `SourceRef` を実装した。track 配列順は下→上。同一 track の重複は `CLIP_OVERLAP`、端点で接する配置は許す。ClipId が同じ Composition の配置を区別し、source_in と TimeMap が独立した local time を決める。Sequence / Track / Clip の UUID 重複・scene ID との衝突、欠落 source、source bounds / map domain を検証する。未知 Sequence / Track / Clip / SourceRef は Sequence 全体を opaque に保持し、通常編集と選択対象の最終レンダーを拒否する。初期の動画描画 source は Composition のみ。Asset 音声の unity-speed ミックスは実装し、Asset / Generator 動画描画、リンク連動編集、clip effects は後続範囲。[ADR-0051](../adr/0051-nle-placement-and-retime.md)、[検証](../testing/nle-001.md) を参照。

初期の SceneNode 種類は Group、Null、Shape、Text、Media、CompositionInstance とする。
Mask / Matte は入力参照として表現でき、見えるレイヤーとして重複描画しない。Repeater / Particles / Scene3D は拡張種類とする。

## ID とインスタンス

NodeId や PropertyId を配列番号や名前から導出しない。表示名の変更で参照は変わらない。
同じ Composition を複数回使うため、実行時の参照キーは概ね `(InstancePath, NodeId, PropertyId)` とする。
InstancePath は、親からたどった CompositionInstance の安定 ID 列であり、配列の現在位置ではない。
共有定義を編集する操作と、公開入力を上書きする操作を別 API にする。

COMP-001 の文書型では、配置ノードの `NodeId` と `CompositionInstanceId` を分け、後者の列を `InstancePath` に保存する。`nodes` の保存順と所有する子の順序を分離し、`root_nodes` と各ノードの `child_order` を順序付き NodeId 列とする。`containment_parent` とこの列の一致を検証し、`transform_parent` は描画順に影響させない。Shape / Text は `ContentId` で内容を参照する。VEC-001 は `Project.shapes` の意味的形状、TEXT-001 は `Project.texts` の UTF-8 本文・style・固定フォントと評価 Property の参照を実装した。詳細は [04 ベクター・日本語テキスト・レイアウト](04-vector-text-layout.md)。GPU の coverage 描画との接続は後続タスク。

読取・変更後は `validate_compositions` で定義集合を検証する。所有・変換の循環はそれぞれ `ContainmentCycle` / `TransformCycle` と閉じた NodeId 経路、定義参照の循環は `CompositionReferenceCycle` と参照元・参照先・Node / Instance を含む辺列で診断する。入力束縛は参照先 Composition の既存 Property を上書きする値源として保持し、型・範囲等の契約を照合する。公開入力・テンプレート方針、Scene IR と評価はこの文書型の実装範囲に含めない。

複数プロセスが同時に編集するため（[09 保存と同時編集](09-storage-concurrency.md)）、ID は中央の採番に依存せず、各プロセスが衝突なく生成できる形式にする。

## 版と互換性

- `schema_version` は保存構造の版。
- `semantic_version` は補間・合成などの意味の版。
- 各 effect / template の version は実装依存を区別する。

RenderSnapshot は公開 `schema_version` を持ち、`semantic_versions` に文書の `semantic_version` と利用する補間・TimeMap・組版・色処理などの意味の版を固定する。revision・エンジン版とこれらの版を同一視せず、実行側の最新で補わない（[ADR-0045](../adr/0045-snapshot-compatibility-boundaries.md)「版の境界」「RenderSnapshot」）。

未知の機能は保存時に失わない設計にするが、必要な機能が不足している場合の最終レンダーは `UNSUPPORTED_FEATURE` で拒否する（[ADR-0010](../adr/0010-unsupported-features-fail-final-render.md)）。

対応する外枠内の未知内容は opaque に保持する。構造を安全に保持できない版は原本を変更せず型付き互換性エラーで拒否し、未知の意味に依存する変更は許さない。保存可能性と実行可能性の判定は ADR-0045 に従う。

保存の具体的な形態（`.kronello`、イベント、スナップショット）は [09 保存と同時編集](09-storage-concurrency.md) を参照。

## 意味的スナップショットと GPU 資源の分離

文書モデルと RenderSnapshot は意味的な値だけを持つ。`wgpu::Texture` や `AVFrame` など、バックエンド・GUI・GPU 資源の寿命に依存する型を保持しない（[ADR-0005](../adr/0005-semantic-snapshot-vs-gpu-resources.md)）。

Property の単位・座標系・範囲は ADR-0043 に従う。保存 Color は色空間タグ付きの straight RGB と独立 alpha とし、内部画像の premultiplied 表現とは区別する（[ADR-0044](../adr/0044-color-and-alpha-contracts.md)）。

## AUDIO-003 の Media と volume

`Clip.volume` は optional `kronello.audio.volume` Property（省略 / null は unity）。
非負有限の dimensionless Scalar Gain を Constant / Curve から純粋評価する。
`NodeKind::Media` は `MediaNode { asset, stream_index, source_in, time_map, volume }` を保存し、
volume は同じ SceneNode の properties にある volume PropertyId。
Media の音声と CompositionInstance の再帰音声を文書音声としてコンパイルする。
Video / Image Media の描画は COMP-002 まで型付き未対応。
Audio track も Composition source を持てる。Video CompositionClip は参照先の音声を一度継承する。
出力 mode、時間写像、trim の sample phase と編集規則は
[ADR-0063](../adr/0063-document-audio-and-clip-volume.md) と [基本音声](audio-000.md) を参照。
