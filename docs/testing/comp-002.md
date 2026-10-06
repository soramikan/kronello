# COMP-002 検証

契約: [ADR-0085](../adr/0085-composition-media-and-template-slots.md)。

## 再現

```sh
cargo test -p kronello-media --test composition --locked
cargo test -p kronello-service --test template media_slots_validate_refs_and_draw --locked
cargo test -p kronello-audio --test document --locked
cargo test -p kronello-audio --test audio4 --locked
cargo clippy -p kronello-render -p kronello-media -p kronello-service --all-targets --locked -- -D warnings
```

Metal が sandbox に公開されない環境では device access を許可して composition suite を実行する。
GPU adapter がない場合は失敗させ、skip / fallback しない。

## 確認範囲

- native PNG16 の 1001 / 2002 / 3003 / 32768 値が RGBA8 へ丸められず、linear / premultiplied alpha として CPU / GPU へ接続する。
- sRGB PNG8、grayscale、palette + alpha の source 数値意味を確認し、Rec.709 / Rec.2020 working primaries の差も確認する。
- unsupported HDR color tag / codec、locked format / dimensions mismatch は型付き失敗。
- Image asset の missing / hash mismatch と、旧 snapshot の missing visual media pin / 未知版を拒否する。欠落 pin の serialize/hash は補わない。
- native ProRes の二色生成 video をネストした Composition へ配置し、active-range 相対 TimeMap / source_in の合成で source time `5/96` を得る。その時刻を直接 decode した CPU oracle と全体 frame を一致させ、明示 GPU と `2^-10` 以内で比較する。source の区間外は `FRAME_NOT_FOUND`。
- Template MediaSlot の default 赤に対する instance override 青が最終 CPU / GPU frame に現れ、各 InstancePath の束縛を使う。
- 共通 service の template.preview（画素あり）と render.frame が同じ slot override を描画する。更新された slot 素材は preview diagnostic `ASSET_HASH_MISMATCH`、frame null。
- Image / visual-only video stream は document audio に配置しない。既存 document audio / AUDIO-004 の回帰も実行する。

## 実行記録

2026-10-06、macOS、Rust 1.95.0、branch `codex/m4-completion`（base `8375caf`）。
composition suite 5 passed（native Metal）、audio document 6 passed、AUDIO-004 12 passed。
共通 service の最終 slot override / preview（画素あり）/ 素材 hash mismatch テスト 1 passed（24.59s）。
対象 crate の all-target clippy `-D warnings` は exit 0。
workspace 全体の最終 gate は統合記録に記載する。
