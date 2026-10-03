# CACHE-001 の検証

`kronello-render` の 4 level cache を GPU adapter なしで検証する。CPU の float32 参照を明示選択する。組版には固定 Noto Sans CJK JP fixture を使用する。

## 受け入れ条件とテスト

以下は `crates/kronello-render/tests/render.rs` の通常テスト。各変更の前に cache を warm にし、counter だけをリセットして changed / unchanged ノードの hit / miss を照合する。

| 条件 | テスト |
|---|---|
| Position / Opacity / Rotation / Scale と別時刻で layout を再利用 | `cache_position_opacity_rotation_scale_reuse_layout_across_renders_and_frames` |
| 色変更は当該 text の raster のみ miss、layout / geometry と別 text / shape は hit | `cache_color_invalidates_only_changed_text_raster` |
| 本文・size・wrap・line_height・alignment は当該 layout / glyph geometry / raster のみ miss | `cache_text_size_wrap_line_height_alignment_invalidate_only_affected_layout_and_downstream` |
| font lock 変更は同一 outline でも当該 layout と下流のみ miss | `cache_font_lock_change_invalidates_downstream_even_with_identical_outlines` |
| shape の色、ROI 平行移動、解像度変更を段階ごとに区別 | `cache_shape_color_and_output_mapping_invalidate_at_their_own_levels` |
| values は snapshot 全内容・runtime property・有理数 Time を区別し、revision だけなら再利用 | `cache_values_use_content_runtime_property_and_exact_time_excluding_revision` |
| cached / zero-capacity / direct CPU の画素・RGBA16F byte 一致、逆順・eviction・clear・ROI / 解像度 | `cache_state_order_eviction_clear_resolution_match_direct_cpu_execution` |
| warm cache でも font 欠落・破損・重複は型付きエラー | `cache_warm_layout_still_rejects_missing_corrupt_and_duplicate_fonts` |
| 連番の画素ファイル・metadata・manifest の byte 一致と frame 間 hit | `cache_sequence_matches_uncached_artifacts_and_reports_cross_frame_hits` |

`src/cache.rs` の unit test `lru_promotes_hits_evicts_oldest_and_enforces_byte_and_entry_limits` は hit による LRU 昇格、entry / byte 上限、過大 entry の非保持を検証する。`failed_raster_computations_are_not_cached_and_namespaces_are_distinct` は失敗の非保持と backend namespace の区別を検証する。

font 変更テストは元の font bytes に有効な末尾 byte を足し、別 SHA-256 の `FontRef` を `pin_font` で取得する。glyph 自体が変わらなくても lock hash の変更が下流へ伝わることを検証する。別フォントファミリーの実機比較を実行したという意味ではない。

## 再現コマンド

指定 worktree の workspace root で実行する。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_
python3 scripts/backlog.py check
```

追加 dependency はないため、Cargo.lock は変更しない。GPU を要する新規テストは追加していない。上記は GPU / framebridge の実機検証、性能測定、GPU golden の保証にはならない。


2026-10-03 の指定 worktree / macOS arm64 / Rust 1.95.0 で、上記 fmt / Clippy / workspace test を実行して通過した。workspace test は合計 252 件成功、失敗 0 件、`gpu_` 2 件除外。`kronello-render` は unit 2 件と CPU integration 19 件（うち CACHE-001 追加 9 件）が成功した。backlog の render / check も通過した。GPU adapter を使う実行は行っていない。
