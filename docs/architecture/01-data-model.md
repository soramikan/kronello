# 01 データモデル

## 最小オブジェクト

| オブジェクト | 主なフィールド |
|---|---|
| Project | schema_version, semantic_version, assets, sequences, compositions, templates |
| Asset | id, content_hash, kind, stream_metadata, immutable_locator |
| DataAsset | id, schema, content_hash, values, time_mapping, analyzer_version |
| Sequence | id, extent, frame_rate, audio_rate, working_space, tracks |
| Clip | id, source_ref, timeline_range, source_in, time_map, links, effects |
| Composition | id, duration, design_extent, edit_rate, root_nodes, properties, inputs, markers, output_ports |
| SceneNode | id, kind, containment_parent, transform_parent, child_order, active_range, transform_ref, content_ref |
| CompositionInstance | id, definition_ref, input_bindings, local_time_map, seed |
| Property | id, type, units, source, modifiers, validation, capabilities |
| AnimationCurve | id, value_type, keys, interpolation_version |
| TemplateDefinition | id, version, composition_ref, public_inputs, duration_policy, constraints |
| RenderSnapshot | content_hash, revision, asset/font/data locks, semantic_versions, profile |

SourceRef は Asset、Composition、Generator を区別する。SourceRef の型が増えても Clip の編集意味は変えない。

初期の SceneNode 種類は Group、Null、Shape、Text、Media、CompositionInstance とする。
Mask / Matte は入力参照として表現でき、見えるレイヤーとして重複描画しない。Repeater / Particles / Scene3D は拡張種類とする。

## ID とインスタンス

NodeId や PropertyId を配列番号や名前から導出しない。表示名の変更で参照は変わらない。
同じ Composition を複数回使うため、実行時の参照キーは概ね `(InstancePath, NodeId, PropertyId)` とする。
InstancePath は、親からたどった CompositionInstance の安定 ID 列であり、配列の現在位置ではない。
共有定義を編集する操作と、公開入力を上書きする操作を別 API にする。

複数プロセスが同時に編集するため（[09 保存と同時編集](09-storage-concurrency.md)）、ID は中央の採番に依存せず、各プロセスが衝突なく生成できる形式にする。

## 版と互換性

- `schema_version` は保存構造の版。
- `semantic_version` は補間・合成などの意味の版。
- 各 effect / template の version は実装依存を区別する。

未知の機能は保存時に失わない設計にするが、必要な機能が不足している場合の最終レンダーは `UNSUPPORTED_FEATURE` で拒否する（[ADR-0010](../adr/0010-unsupported-features-fail-final-render.md)）。

保存の具体的な形態（`.koma`、イベント、スナップショット）は [09 保存と同時編集](09-storage-concurrency.md) を参照。

## 意味的スナップショットと GPU 資源の分離

文書モデルと RenderSnapshot は意味的な値だけを持つ。`wgpu::Texture` や `AVFrame` など、バックエンド・GUI・GPU 資源の寿命に依存する型を保持しない（[ADR-0005](../adr/0005-semantic-snapshot-vs-gpu-resources.md)）。
