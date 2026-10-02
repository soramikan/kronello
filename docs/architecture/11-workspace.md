# 11 ワークスペースと実装責務

状態: 構成案。`crates/` と `apps/` はまだ作成していない。

初期は以下を論理モジュールとして開始し、ビルド依存やテスト境界に応じて crate 分割する。過度な micro-crate 化はしない。

```text
crates/
  koma-model/          # IDs, document types, property descriptors, versions
  koma-time/           # rational time, ranges, TimeMap, sampling
  koma-animation/      # curves, interpolation, modifier contracts
  koma-expr/           # typed AST, dependencies, bounded evaluator
  koma-scene/          # composition, parenting, masks, Scene IR
  koma-layout/         # responsive constraints, metrics, bounds
  koma-text/           # fonts, Japanese layout, glyph/cluster mappings
  koma-vector/         # paths, shape IR, geometry operations
  koma-render/         # DAG compiler, region/time planner, scheduler
  koma-gpu/            # wgpu, pipelines, color/alpha, texture pools
  koma-media/          # FFmpeg integration, seek, decode/encode
  koma-framebridge/    # OS/GPU specific interop and synchronization
  koma-audio/          # mixer, buses, feature-data integration
  koma-store/          # SQLite, snapshots, migrations, event journal
  koma-template/       # typed inputs, bindings, duration, versions
  koma-service/        # commands, queries, policies, job orchestration
  koma-cli/            # machine-oriented CLI adapter (binary: koma)
  koma-mcp/            # MCP adapter
  koma-ffi/            # FFI boundary for native GUI apps
apps/
  macos/               # Swift (SwiftUI / AppKit) desktop app
  windows/             # 将来
  linux/               # 将来
```

v0.2 仕様からの変更: 接頭辞 `ved-` → `koma-`。`ved-desktop` を廃し、`koma-ffi` と `apps/` に置き換えた（[10 デスクトップ GUI](10-desktop-gui.md)）。

## 依存の向き

```text
model / time
   -> animation / scene / text / vector
   -> render
   -> backend (gpu / media / framebridge / audio)
```

- store / service / UI は評価エンジンを呼ぶが、評価エンジンは store や UI へ逆依存しない。
- GPU texture、AVFrame、SQLite connection、Tokio runtime の型を model に漏らさない。
- FFmpeg や wgpu の API 差分はアダプターで吸収し、`Cargo.lock` と native dependencies manifest を固定する。
- cli / mcp / ffi は service の薄いアダプターとし、編集の意味を持たない。

## 未決事項

MSRV、edition、lint、CI は [OQ-15](../open-questions.md)。
