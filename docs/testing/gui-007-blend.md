# GUI-007: Clip blend の検証記録

状態: コア実装・個別検証・統合checkpointとGUIのmode切替を確認済み（2026-10-06）。残るGUI操作・最終CIは未完了であり、GUI-007全体を完了としない。

## 契約

[ADR-0101](../adr/0101-linear-premultiplied-layer-blend.md)。`kronello.blend_mode` Enum の Normal / Multiply / Screen を既存 Clip properties と shared `ClipSetEffects` で保存する。省略は Normal。source の isolation / opacity / effects / matte の後に sibling backdrop と線形 premultiplied 合成する。HDR RGB は clamp しない。

## 確認済み

- `cargo test -p kronello-model --test builtin_registry -p kronello-service --test blend --locked`: builtin 14 tests、blend 2 tests 成功。固定 descriptor identity / lexical enumeration、未知 mode / Curve 拒否、重複 mode の project 拒否、実 shared plan / apply / 保存 / idempotent retry / revision conflict / Undo を確認。2 tracks の solid generator Clip と placement opacity の実 render で Normal / Multiply / Screen の RGB / alpha を独立の数値期待値と比較した。legacy pin 省略、非 Normal の pin 欠落 / 未知 pin 拒否、mode を含む raster cache identity も確認。
- `cargo test -p kronello-gpu --test scene cpu_linear_blend --locked`: CPU の実 DrawScene を透明 source/backdrop・partial alpha・HDR 1 超 / 負値で数値期待値と比較し成功。
- `cargo test -p kronello-gpu --test scene gpu_linear_blend --locked -- --nocapture`: Apple M4 / Metal 実機で成功。Rec.709 / Rec.2020、3 modes、transparent / HDR を isolation・opacity・GaussianBlur・alpha matte の後に合成し CPU と比較した。sandbox 外で実行し、adapter fallback / skip はしていない。
- `cargo check -p kronello-service -p kronello-gpu --locked`: 成功。
- `cargo clippy -p kronello-service -p kronello-gpu --all-targets --locked -- -D warnings`: 成功。その後の全workspace検証も下記checkpointで確認した。

## 統合とGUIの追加確認

先行targeted Clippyは、並行中の `DescriptorDefinition.repeatable` 追加との不整合で失敗した。その後、公開schema / GeneratedAPI再生成、全workspace fmt / Clippy / testは [M5 checkpoint](m5-acceptance.md) で成功した。先行失敗を現在の未解消エラーとは扱わない。

rootがmacOSアプリの `edit-controls-007-full` fixtureでNormal→Multiply（rev 2）→Screen（rev 3）を操作し、合成表示と保存を確認した。ローカル画像は `target/m5-acceptance/gui/edit-multiply.png` / `edit-screen.png`。Multiply画像には更新中表示を含むため、完了後の厳密な画素比較の証拠にはしない。

## 残件

- mode変更に固有のGUI Undo・外部変更・競合、実CLI/MCP transport同等性。一般の競合試験成功だけでこれらを完了にしない。
- このcheckpoint後の変更に必要な回帰確認とM5最終CI。

同時期の MATTE-001 検証で発見した INSPECT-001 既存 test の期待値を ADR-0099 の `MATTE_MISSING` へ整合した。単なる code 置換ではなく、inactive matte/source の理由の provenance、plan=null、最終 render DAG の同一型付き失敗、文書非変更を確認し、service inspect 全13 tests が成功した。
