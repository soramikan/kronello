# PR #19 レビュー修正の検証

2026-10-12、`probe_for_import` の RAW 拒否判定を probe より前へ移した。既存の公開 `probe` 自体も RAW を先に sniff するため、レビュー要約の「FFmpeg が RAW sniff より先に走る」はその経路には該当しなかった。ただし import 専用の診断が RAW デコーダの成否に左右される順序は改善した。

BRAW / R3D は既存の vendor SDK 診断、RAW still / ProRes RAW は明示 RAW registration workflow の `UNSUPPORTED_FEATURE` を返す。壊れた DNG / CR2 もデコードを試みる前に拒否する。通常の A/V import は既存の probe・初回 decode 検証を維持する。

macOS のローカル検証:

- `cargo test --locked -p kronello-media --test raw`: 9 passed。BRAW、R3D、正常/壊れた DNG、壊れた CR2、ProRes RAW の import 拒否を含む。
- `cargo test --locked -p kronello-service --test media_import`: 3 passed。動画・音声・画像、複数stream、hash、receipt replay、undo を確認。

前段 PR #17 の修正も merge commit で取り込んだ。workspace と各 OS の受け入れは、この修正を含む最新 PR CI の結果を正本とする。
