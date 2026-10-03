# TEMPLATE-001 検証

## 実装と判断

M2 は `TemplateDefinition` と `TemplateInstance` を別コレクションに保存する。
定義の `id` は版の ID、`template_id` は系列の ID、`version` は固定文字列。
instance は定義 ID と同じ version を持ち、入力値だけを上書きする。
`template.define` は同じ版の再公開を `TEMPLATE_VERSION_EXISTS` で拒否する。
公開入力の既定値だけを変える新版は同じ Composition を参照できる。
内部の Composition・Text・Shape・Curve を変える新版は、それぞれ新しい ID を持つ内容を作ってから定義する。
到達する authoring 内容の `content_hash` を定義時に固定し、編集・import・レンダーで照合する。
入れ子の template の定義・固定版・入力上書きも hash に含める。
既存版の変更は `TEMPLATE_DEFINITION_CHANGED`。版移行、差分計画、variant、data、hold / loop は TEMPLATE-002 に残す。

未知の template 定義・instance は opaque のまま共通 service の create / import / export で保持する。
編集は拒否し、最終レンダーでは選択 Composition の依存閉包に必要な未知 template だけを `UNSUPPORTED_FEATURE` にする。
独立した未知定義は snapshot の identity に含めるが、その存在だけでは既知出力を拒否しない。
公開済みの既知定義や到達内容を未知フィールドで opaque に変え、hash 照合を回避する import は `TEMPLATE_DEFINITION_CHANGED`。

尺は `stretch`。5 秒の定義、intro `2/5` 秒、outro `3/10` 秒を 8 秒へ写す制御点は、instance→definition の順に
`(0,0), (2/5,2/5), (77/10,47/10), (8,5)`。
区間内は有理数の区分線形補間、intro / outro の傾きは 1。
`minimum_middle` と保護区間の合計未満、または中間区間が空になる要求は安全上 `DURATION_TOO_SHORT`。
placement は instance の `[0,duration)` に置く。配置開始位置の編集はこの API の範囲外。

背景帯は同じ親空間にある Rectangle の size / position を読む。
`text Property → LayoutValue → band Property` を `DependencyDeclarations` に列挙する。
上位の render compiler がテキストを組版し、`layout_bounds` を親空間に写した矩形へ design_px の padding を加える。
`kronello-eval` は外部の意味的 Vec2 を受け取り、text / template crate に依存しない。
`wrap_width` は独立した text Property。逆依存を追加したグラフは `PROPERTY_DEPENDENCY_CYCLE`。
背景帯の変換は position のみを許す。テキスト入力は水平・単一 style・ruby なしを対象とし、置換時も UTF-8 byte range を再構築する。
TEXT-001 の `layout_bounds` は wrap_width × 行数 × line_height の矩形であり、同じ wrap_width の文字数変更では帯の幅を保ち、折り返しによる行数変更へ高さが追従する。字形の実幅（ink_bounds）に置換しない。
組版前に配置と text の containment 親の active_range を確認し、非アクティブな親の下では組版・overflow 判定・域外の TimeMap 評価を行わない。

`max_lines` を超えた場合は、node、actual、maximum を持つ `TemplateError::Overflow`。
最終レンダーは `TEMPLATE_OVERFLOW` で拒否し、service / CLI でも `details.actual_lines` / `details.max_lines` を返す。
無言のクリップ、文字縮小、正常な出力扱いは行わない。失敗した画像連番は確定ディレクトリへ公開しない。

## 受け入れ条件とテスト

| 条件 | 再現可能なテスト |
|---|---|
| 定義と instance 入力の分離、版固定 | service `edition_pin_input_commands_revision_idempotency_and_undo`、`authoring_edit_cannot_change_a_published_edition`、`instance_duration_edit_rebuilds_map_and_import_cannot_republish_definition` |
| 5秒→8秒、intro / outro 保持 | template `five_to_eight_seconds_preserves_intro_and_outro_sampled_values_exactly`。境界・サブフレームで TimeMap と曲線の値を完全一致比較。CLI / service は既存 instance の `template.set_duration` も検証 |
| 背景帯の単方向追従 | render `japanese_instances_have_independent_text_color_duration_and_following_bands`。実フォントで別文字・色・尺の 2 instance、padding、位置、CPU 画素の存在、評価履歴非依存を検証 |
| 循環拒否 | eval `declared_layout_values_schedule_consumers_and_reject_reverse_wrap_cycle`。wrap / layout / band を含む閉じた循環経路を検証 |
| overflow | render `overflow_is_typed_and_sequence_has_no_published_output`、CLI `template_commands_share_schema_service_and_report_final_overflow`。3行→最大2行、型付き診断、非ゼロ exit、単一 stdout JSON、成果物未公開を検証 |
| 固定 snapshot | render `frozen_snapshot_does_not_read_new_instance_inputs` |
| 入れ子の版・入力の固定 | template `nested_template_inputs_and_definition_are_frozen` |
| 非アクティブな配置親 | render `inactive_placement_parent_skips_layout_and_overflow` |
| 型・範囲、公開範囲 | template `defaults_constraints_and_public_input_isolation`、`bounds_padding_and_typed_overflow` |
| 公開 schema | model の生成 schema 一致テストと CLI の保存済み template document の JSON Schema validation |
| 保存互換性 | model `template_editions_and_separate_pinned_inputs_round_trip`、`legacy_documents_omit_template_collections_and_future_contracts_remain_opaque`、`template_identity_collisions_and_shadowed_collections_are_rejected` |
| 共通 API の未知内容保持と実行境界 | service `future_template_contracts_round_trip_through_create_import_export`、`import_cannot_obscure_published_content_or_definition_with_opaque_fields`、render `independent_opaque_templates_are_preserved_but_required_contracts_fail_render` |

```sh
python3 scripts/fetch_fixtures.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_
python3 scripts/backlog.py check
git diff --check
```

GPU / FrameBridge の実機検証、基本 shadow、4K、MCP、音声、ジョブはこの検証に含めず、INTEGRATION-001 と各担当タスクで確認する。

## 今回の実行結果（2026-10-03）

`m2-template-001`（基点 `9dce6ae`、未コミットの作業ツリー）を sandbox 内の Rust 1.95.0 で確認した。
コンパイル・テストは `CARGO_HOME=/private/tmp/kronello-template-cargo`、作業ツリー既存の target を使用した。

- `cargo fmt --all --check`、workspace Clippy（全 target、`--locked`、警告をエラー化）は成功。
- 上記の GPU / FrameBridge を除く workspace test は **309 passed / 0 failed / 0 ignored**。`gpu_` の **4 tests** を除外した。
- template の純粋契約・service・render の対象テストは **15 passed / 0 failed**。
- CLI 再現スクリプトは終了コード 0。5 秒の配置を 8 秒へ変更し、CPU で 2 frame を出力した。3 行入力は `TEMPLATE_OVERFLOW`、失敗出力ディレクトリは存在しなかった。
- `python3 scripts/backlog.py check`、`git diff --check` は成功。

これらは CPU の意味的結果と保存・CLI 契約の確認であり、GPU 画素の固定環境比較の合格ではない。

## CLI 再現

`examples/template-001.project.json` は 5 秒の authoring Composition と 8 秒の空の配置先を持つ。
`examples/template-001.definition.json` は公開 headline / accent と背景帯・最大2行の定義。
`content_hash` は `template.define` が計算するため入力例では空文字。
全コマンドに project path、base_revision（10進文字列）、session_id、idempotency_key を渡す。
`template.instantiate` は composition / node / index と、版固定された instance を渡す。
`template.set_input` は instance / name / 型付き value、`template.set_duration` は instance / 有理数 duration を渡す。
これらは `EditCommand::Template` として `edit.plan` / `edit.apply` にも含められ、共通の revision・idempotency・Undo を利用する。
CLI の完全な要求と実行例は `scripts/demo_template_cli.py --output-directory <新規ディレクトリ>` で再現する。
