# FRAMEBRIDGE-001: generic VideoToolbox 経路と診断

状態: `done`（M5作業ブランチ）。[統合checkpoint](m5-acceptance.md)を確認済み。

設計: [ADR-0098](../adr/0098-explicit-framebridge-path-inventory.md)。

## 確認する範囲

- `PathKind::VideoToolbox` は互換用の拒否selectorとして保持する。具体経路へのalias・fallback・
  encodeの入口にしない。
- `CONCRETE_PATHS` は CPU upload / GPU copy / readback、IOSurface import / output、
  CVPixelBuffer import、BGRA8 / NV12 decodeの8経路。重複とgenericを含めない。
- `require_gpu_resident` とgeneric `SpikePath::measure` の拒否code・診断を一致させる。
  診断は具体的decode選択肢を示し、encodeは提供しないことを明示する。
- policyの成功をruntime availabilityと呼ばない。非macOSのnative経路拒否は維持する。

## 再現コマンド

`cargo test -p kronello-framebridge --test paths --locked`

`cargo test -p kronello-framebridge --test videotoolbox --locked -- --ignored --nocapture`

`cargo clippy -p kronello-framebridge --all-targets --locked -- -D warnings`

具体decodeのnative試験は生成したH.264を使う既存probeであり、generic decode/encodeや
全codec/containerの保証へ読み替えない。実行結果は以下の通り。

## 実行結果（2026-10-06）

Apple M4 / macOS27.0.1（26A434）/ Metal実機で上記paths + videotoolboxを `--include-ignored --nocapture` により
実行し、paths 5件・native probe 3件が成功した（exit 0）。sandbox内ではMetal adapterが
見つからずGPU試験が失敗したため、実機アクセスを許可したホスト実行で確認した。

- generic selector: residency policyとmeasurementの同じ型付き拒否を確認。
- BGRA8 strict decode: H.264 64×64の3 frame、hardware decoder=true、query OSStatus=0、
  報告pathはBGRA8、最大channel誤差1。NV12へ置換しない。
- NV12 explicit decode: 同じ3 frame、hardware decoder=true、query OSStatus=0、
  報告pathはNV12。R8/RG8 plane bytes検証であり、BGRA色比較の成功とは扱わない。
- 両decode probeのCPU pixel upload・GPU copyは0、検証readbackは3 operation / 49,152 bytes。
  最終readbackを含めたzero-copyとは呼ばない。
- CVPixelBuffer import: CPU upload / GPU copyは0、検証readback512 bytes。
- framebridge all-targets Clippy `-D warnings` 成功（exit 0）。

新しいcodec・pixel formatを追加した試験ではない。resident本番の保証境界はGPU-003を維持する。

## 統合受け入れ

2026-10-06にタスク固有条件と [M5統合checkpoint](m5-acceptance.md) を確認し、`done` とした。M5最終変更の各OS CIはマイルストーン全体で別途確認する。
