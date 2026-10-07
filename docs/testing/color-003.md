# COLOR-003 `.cube` 3D LUT の取り込み・検証・適用の検証

現在の状態（2026-10-08）: M8 Lane B の作業ブランチ `m8-lane-b` で実装・検証を完了した。コードのコミットは本書と同じコミット。`docs/backlog/backlog.json` の状態更新はこの文書の範囲外。

## 契約

[ADR-0113](../adr/0113-lut-import-and-scope-observation.md) に従い、IRIDAS `.cube` の 3D LUT を versioned エフェクト `kronello.color.lut` version **1** として実装した。

- `.cube` パーサは `kronello_model::lut`（`crates/kronello-model/src/lut.rs`）に置く純粋バイトパーサ。`LUT_3D_SIZE 2..=65`、`TITLE`、コメント、`DOMAIN_MIN` / `DOMAIN_MAX`、RGB 行を受理する。document/render 側の上限 `N <= 33` は `validate_document_size` で別段検査する。`LUT_1D_SIZE` のみの文書は `UNSUPPORTED_FEATURE`、不正文書は `INVALID_LUT` の型付きエラー。
- LUT は外部 `Asset`（`AssetKind::Data`）として保存し、格子データを作品文書に展開しない。`lut.import` はファイルバイトを読み、sha256 `content_hash` を自前で計算して記録する（要求側の hash は信用しない）。locator はプロジェクト直下なら相対、それ以外は絶対パス。
- エフェクトパラメータは `lut`（`ValueType::AssetRef`、descriptor id `0xf0000000-0010-4200-8000-000000000001`）と `intensity`（scalar `0..=1`、同 `...0002`）。`EffectDefinition::validate` / `resolve` が所有・型・値域を検査する。
- 画素演算は作業空間の unpremultiplied RGB をドメイン正規化して四面体補間し、範囲外は端点色へ clamp、alpha は不変、`straight + (mapped - straight) * intensity` で原画像とブレンドする。CPU oracle は `kronello_gpu::color`、WGSL は `effect.wgsl` の if-chain 実装（switch / 動的 index なし、FXC 互換）で同じ式を共有する。LUT バイトは `RenderInput.luts`（hash → パス）で渡し、sha256 を検証してから lattice を結び付ける。
- 参照先が未指定・欠落・種別違い・格子不正なら `LUT_INPUT_MISSING` / `INVALID_LUT` 等の型付きエラーでレンダーを止める。エフェクトは著者順に pointwise 適用される。

## 受け入れ条件との対応

| 条件 | テスト・手順 | 状態 |
|---|---|---|
| `.cube` の取り込み・検証・作品内参照 | `lut.import`（Data asset + content_hash、service `lut_import_verifies_content_and_drives_render_inputs`） | CPU 合格 |
| パーサの受理範囲と型付き拒否 | model `lut.rs` 内テスト（identity・domain・malformed・1D-only・size 上限・document cap 分離） | CPU 合格 |
| HDR/SDR 変換パイプラインでの適用位置の規約 | 作業空間 straight RGB + 端点 clamp + alpha 不変を ADR-0113 に固定、CPU/GPU テストで検証 | CPU/GPU 合格 |
| resolve/型/値域エラー | model `color003_lut_resolves_asset_reference_and_bounded_intensity`、descriptor pin `color003_lut_descriptor_types_and_versions_are_pinned` | CPU 合格 |
| 著者順・premultiply・alpha 保持・intensity | render `color003_lut_applies_tetrahedral_in_authored_order_and_preserves_alpha`、`color003_intensity_blends_toward_identity`、GPU `cpu_color003_lut_samples_straight_rgb_preserves_alpha_and_clamps_domain` | CPU 合格 |
| Scene IR / DAG / snapshot の lattice 結び付け | render `color003_scene_ir_binds_referenced_luts_and_dag_holds_tetrahedral_node` | CPU 合格 |
| 欠落・hash 不一致・不正 lattice の型付き失敗 | render `color003_missing_or_broken_lattice_is_a_typed_render_failure`、service `lut_import_verifies_content_and_drives_render_inputs`（`LUT_INPUT_MISSING` / `ASSET_HASH_MISMATCH` / `LUT_MISSING`） | CPU 合格 |
| 1D-only / 不正 / oversized 取り込み拒否 | service `lut_import_rejects_malformed_and_unsupported_documents`（`UNSUPPORTED_FEATURE` / `INVALID_LUT`） | CPU 合格 |
| CPU / GPU 意味的一致（両作業空間） | GPU `gpu_color003_lut_matches_cpu_reference_in_both_spaces` | GPU 実測合格（下記） |
| GUI オーサリング（asset picker + intensity + `clip_set_effects`） | `ClipColorInspector` の LUT 行と `LutEditor`、`EditorModel.addClipLutEffect` / `importLut` / `lutAssets` | ビルド合格・手順は下記 |

## 実行記録

検証環境: Apple M4（arm64）、macOS、Metal、rustc 1.95.0（`rust-toolchain.toml`）、`Cargo.lock` 固定。この worktree では `python3 scripts/fixtures.py generate` で media fixture を `target/fixtures/generated` に生成した（未追跡の検証入力のみ）。fixture 未生成のままでは NLE-002 video 系テストが環境要因で失敗するため、再現には先に fixture 生成が必要。

- `cargo test -p kronello-model --test color003`: exit 0、2 passed。
- `cargo test -p kronello-render --test color003`: exit 0、4 passed。
- `cargo test -p kronello-service --test color003`: exit 0、3 passed。
- `cargo test -p kronello-gpu --test scene` の COLOR-003 系 2 件: exit 0。`gpu_color003_lut_matches_cpu_reference_in_both_spaces` は回転格子（size 5、非既定ドメイン）で部分 alpha・HDR・ドメイン外入力 × intensity 0.35/1.0 × LinearRec709/LinearRec2020 を CPU oracle と一致確認。
- `cargo test --workspace --locked`: exit 0（ignored は物理 encoder・GPU 等の既存保留のみ）。
- `cargo fmt --all --check`: exit 0。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0。
- `python3 scripts/generate_swift_api.py --check`: exit 0。`schemas/api-v1.schema.json` / `GeneratedAPI.swift` は generator 出力と一致、`schemas/project-v1.schema.json` は `cargo run -p kronello-model --example project_schema` で再生成した。
- `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --package-path apps/macos --disable-sandbox`: exit 0。ビルドには `python3 scripts/build_ffi.py` で `apps/macos/Libraries/libkronello_ffi.dylib` と CLI を先に生成する必要がある。

## GUI 手動手順

1. Edit ページで映像クリップを選択し、Inspector「カラー」→「LUT」行の `.cube を読み込む`（+ ボタン）で `.cube` を取り込む（`lut.import`、Data asset として登録）。
2. 「LUT を追加」のポップアップで asset を選ぶと `kronello.color.lut` v1 が `clip_set_effects` で追加される。
3. 「LUT」ポップアップで参照 asset を差し替え、「強度」フィールドで `0...1` の intensity を編集する。参照先 asset が欠落した場合は警告表示になり、レンダーは型付きエラーで停止する。

残件: golden baseline への LUT シーン追加はこの lane では行わない（他 lane との golden カタログ競合を避け、意味的一致は CPU/GPU 等価テストで担保した）。CLI / MCP は共有 API 経由のため `lut.import`・`inspect.scopes`・`luts` 入力がそのまま利用できる。
