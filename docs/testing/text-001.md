# TEXT-001 の検証

横書き日本語、固定フォントと欠落検査、元 UTF-8 範囲から書記素 / shaping cluster / glyph への対応を CPU の通常テストで確認する。GPU 画素比較ではない。

固定入力は `resolve_fixture("japanese")` と `resolve_fixture("noto-sans-cjk-jp")`。日本語 JSON は生成元 `scripts/fixtures.py canonical_data`、byte 数、SHA-256、manifest と台帳を合わせて更新した。追加した `multi-glyph-grapheme`（`x + U+3099`）と `ligature`（`office`）も MIT OR Apache-2.0 の生成入力。フォント本体・OFL・固定 hash は変更していない。

| 受け入れ条件 | テスト（`crates/kronello-text/tests/layout.rs`） | 比較内容 |
|---|---|---|
| 横書き・design_px 幅の折り返し・基本禁則 | `horizontal_japanese_wraps_with_basic_kinsoku` | 20 design_px の本文、幅 60 で `「日本` / `語」、` / `句読` / `点。` の 4 行、各 advance と bounds |
| 長音・小書き・括弧の禁則 | `small_kana_long_mark_and_parentheses_do_not_create_prohibited_soft_breaks` | 全 soft line 境界の禁止文字、分割不能区間の明示 overflow |
| フォント固定 | `fixed_font_identity_matches_manifest_and_font_names`、`missing_font_hash_mismatch_metadata_and_face_mismatch_are_typed` | 実 bytes の hash・family・PostScript 名、欠落・改変 bytes・名前違い・face index 不正 |
| glyph 欠落を列挙して拒否 | `missing_emoji_and_unmapped_ivs_list_whole_source_clusters_without_fallback` | ZWJ emoji、U+10FFFF、未対応 IVS を元範囲と文字列で報告し、fallback 結果を返さない |
| 書記素と glyph の非一対一 | `one_grapheme_maps_to_multiple_glyphs_as_one_animation_unit` | `x + U+3099` は 1 書記素、1 shaping cluster、2 glyph、1 AnimationUnit |
| 複数書記素と 1 glyph | `ligature_maps_multiple_graphemes_to_one_shaping_cluster` | `office` の `ffi` は 3 書記素→1 glyph、元 byte range `[1, 4)` を保持 |
| 結合濁点・IVS | `combining_dakuten_and_ivs_preserve_original_source_and_glyph_selection` | 非正規化入力を保持し、濁点は合成済み `が` と同 glyph、IVS は `葛` 単独と異なる指定 glyph |

すべての正常な代表入力で `assert_mapping` が元 range の連続・UTF-8 境界、書記素から cluster への参照、glyph の逆対応、AnimationUnit の cluster 分割禁止を確認する。組版テストは計 20 件。追加の regional indicator / Hangul jamo 入力は、shaper と Unicode 書記素分割の差があっても source cluster を壊さないことを確認する。他の通常テストは明示改行と空行、alignment による Path と ink の移動、複数 style のサイズ・色、layout/ink bounds の区別、outline の有効性、異体字、予算・非有限値、不変入力と履歴に依存しない再実行、縦書き / ルビ / 未知版 / control の明示拒否を扱う。

`crates/kronello-model/tests/text.rs` は 8 テストで Project の往復と旧文書の `texts` 省略、不正 style・FontRef・ruby range、内容と Property の参照 closure、重複 ID、未知フィールドの opaque 往復、未知意味版の編集拒否、design_px と正数 descriptor、最終評価値の拒否を確認する。公開 Schema の一致は既存の `crates/kronello-store/tests/storage.rs` の `committed_schema_matches_rust_types` で確認する。

## 再現手順

worktree / checkout の root で、Rust 1.95.0 を使う。

```sh
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py check
cargo run -p kronello-model --example project_schema --locked > schemas/project-v1.schema.json
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked
python3 -m unittest discover -s scripts/tests -v
python3 scripts/backlog.py check
```

必要なら各 cargo に `CARGO_HOME=/private/tmp/kronello-text-cargo` を付ける。フォント未取得・hash 不一致は失敗する。取得不要の検証は `fetch_fixtures.py --offline`。GPU / FrameBridge のテストは別環境で supervisor が実行する。

未実装: ルビ・縦書き、可変フォント軸、完全な bidi、単語辞書、高度な selector、永続 cache、GPU coverage 描画、template の overflow 許可 / 拒否方針。明示改行は禁則より優先し、分割不能区間は overflow を返す。RenderSnapshot と組版意味版の対応は後続 compile で接続する。CPU の意味的テスト通過を GPU 描画・多 OS 実測の保証と扱わない。

## 今回実行した結果

2026-10-03、`m1-text-001` worktree、Darwin arm64、rustc 1.95.0 の sandbox で実行。

- `cargo fmt --all --check`: 成功。
- `cargo clippy --workspace --all-targets -- -D warnings`（依存追加後）および `--locked` 付き: 成功。初回の大きいエラー variant の指摘は Box 化で修正済み。
- `cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked`: 229 件成功、0 失敗、0 ignored（Text 組版 20 件、Text model 8 件を含む）。
- 公開 Schema の再生成と `committed_schema_matches_rust_types`: 成功。
- `python3 scripts/fixtures.py check`: 15 fixture、9 scene、28,012 bundled byte を検証。
- `python3 scripts/fetch_fixtures.py --offline`: 固定 Noto を検証。
- `python3 -m unittest discover -s scripts/tests -v`: 13 件成功。
- `python3 scripts/backlog.py render` / `check`、`git diff --check`: 成功。

GPU / FrameBridge、Linux / Windows の実機実行はこの worker では未検証。TEXT-001 の CPU 受け入れ条件を通常テストへ対応付けて `done` にしたが、統合レビュー・merge・commit は supervisor の担当。
