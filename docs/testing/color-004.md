# COLOR-004 スコープ（waveform / vectorscope / histogram / RGB parade）の検証

現在の状態（2026-10-08）: M8 Lane B の作業ブランチ `m8-lane-b` で実装・検証を完了した。コードのコミットは本書と同じコミット。`docs/backlog/backlog.json` の状態更新はこの文書の範囲外。

## 契約

[ADR-0113](../adr/0113-lut-import-and-scope-observation.md) に従い、共有 Command/Query API の読み取りクエリ `inspect.scopes` を追加した。

- 要求は `{ "operation": "inspect.scopes", "input": <RenderInput>, "time": <Rational> }`。`RenderInput` の composition / sequence ターゲット・region・`fonts`・`luts` をそのまま使う。スコープは既存のレンダー評価経路（`Service::render_requested_frame`）を 1 回だけ通した合成済み作業空間フレームを観測し、独立した評価パイプラインは持たない。バックエンドはサービス側で `cpu_reference` に固定し、呼び出し側に選ばせない（同一入力は常に同一の bin）。
- 出力は `InspectScopesResult`（`revision`・`time`・`working_space`・`size` + 4 系統の整数 bin）。`kronello_render::compute_scopes`（`crates/kronello-render/src/scopes.rs`）が straight RGB に逆 premultiply し、luma / Cb / Cr を working-space の係数で計算して固定解像度に binning する:
  - waveform / parade: 512 列 × 256 段（parade は R/G/B 3 面）
  - vectorscope: 256×256（`bins[cr * size + cb]`、neutral は中央）
  - histogram: R/G/B/luma 各 256 bin
  - 範囲外（負値・HDR）は端 bin へ clamp、alpha ≈ 0 は黒として扱う。非線形の作業空間要求は `INVALID_INPUT` の型付きエラー。
- GUI 側の可視化は bin をそのまま描くだけで、display transform は render/inspect 層に入れない。

## 受け入れ条件との対応

| 条件 | テスト・手順 | 状態 |
|---|---|---|
| 4 系統のスコープ生成と整数 bin | render `compute_scopes` 実装 + `scopes_are_deterministic_and_count_every_pixel`（画素数保存） | CPU 合格 |
| 端 bin clamp・zero alpha・次元不一致・非線形空間の拒否 | render `zero_alpha_and_extreme_values_land_in_edge_bins`、`dimension_mismatch_and_encoded_working_space_are_typed` | CPU 合格 |
| 共有クエリ・決定性・revision 付与 | service `inspect_scopes_return_deterministic_bins_with_revision`（同一要求 2 回の完全一致、revision/size/working_space、各 bin 合計が画素数） | CPU 合格 |
| schema / registry / wire 通過 | service `actual_results_for_every_command_match_envelope_and_registry_schemas`、`every_request_payload_and_envelope_matches_schema_and_denies_execution_fields` | CPU 合格 |
| macOS GUI スコープパネル | `ScopesPanel`（折りたたみ、4 系統を Canvas 描画、revision/sequence/playhead で再取得）を Edit ページへ追加 | ビルド合格・手順は下記 |

## 実行記録

検証環境: Apple M4（arm64）、macOS、Metal、rustc 1.95.0（`rust-toolchain.toml`）、`Cargo.lock` 固定。`python3 scripts/fixtures.py generate` で media fixture を生成済み。

- `cargo test -p kronello-render`（`scopes.rs` 内テスト 3 件を含む）: exit 0。
- `cargo test -p kronello-service --test color003`: exit 0、`inspect_scopes_return_deterministic_bins_with_revision` を含む 3 passed。
- `cargo test -p kronello-service --test api`: exit 0（`inspect.scopes` の schema・実実行・wire 往復を含む 12 passed）。
- `cargo test --workspace --locked`: exit 0。
- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`: ともに exit 0。
- `python3 scripts/generate_swift_api.py --check`: exit 0。
- `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --package-path apps/macos --disable-sandbox`: exit 0（事前に `python3 scripts/build_ffi.py` で `libkronello_ffi` を生成）。

## GUI 手動手順

1. Edit ページでシーケンスを開き、ビューア下部の「スコープ」行を開く。
2. 現在の合成済みフレーム（作業空間）に対し `inspect.scopes` を発行し、波形 / ベクトルスコープ / ヒストグラム / RGB パレードを表示する。見出しに評価した `revision` を表示する。
3. 再生ヘッド移動・編集（revision 更新）・シーケンス切替で自動再取得する。評価が失敗した場合は型付きエラーコードを表示する。

残件: bin 解像度は固定（要求側の解像度指定は未実装）。スコープの GUI 画素比較は行わない（描画は決定的 bin の可視化のみ）。縦長など極端なアスペクトでは 256×256 に丸めたピクセル領域で評価する。
