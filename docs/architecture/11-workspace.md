# 11 ワークスペースと実装責務

状態: 構成案。`crates/` と `apps/` はまだ作成していない。

初期は以下を論理モジュールとして開始し、ビルド依存やテスト境界に応じて crate 分割する。過度な micro-crate 化はしない。

```text
crates/
  kronello-model/          # IDs, document types, property descriptors, versions
  kronello-time/           # rational time, ranges, TimeMap, sampling
  kronello-animation/      # curves, interpolation, modifier contracts
  kronello-expr/           # typed AST, dependencies, bounded evaluator
  kronello-scene/          # composition, parenting, masks, Scene IR
  kronello-layout/         # responsive constraints, metrics, bounds
  kronello-text/           # fonts, Japanese layout, glyph/cluster mappings
  kronello-vector/         # paths, shape IR, geometry operations
  kronello-render/         # DAG compiler, region/time planner, scheduler
  kronello-gpu/            # wgpu, pipelines, color/alpha, texture pools
  kronello-media/          # FFmpeg integration, seek, decode/encode
  kronello-framebridge/    # OS/GPU specific interop and synchronization
  kronello-audio/          # mixer, buses, feature-data integration
  kronello-store/          # SQLite, snapshots, migrations, event journal
  kronello-template/       # typed inputs, bindings, duration, versions
  kronello-service/        # commands, queries, policies, job orchestration
  kronello-cli/            # machine-oriented CLI adapter (binary: kronello)
  kronello-mcp/            # MCP adapter
  kronello-ffi/            # FFI boundary for native GUI apps
apps/
  macos/                     # Swift (SwiftUI / AppKit) desktop app
  windows/                   # 将来
  linux/                     # 将来
```

v0.2 仕様からの変更: 接頭辞 `ved-` → `kronello-`。`ved-desktop` を廃し、`kronello-ffi` と `apps/` に置き換えた（[10 デスクトップ GUI](10-desktop-gui.md)）。

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

## ツールチェーンと CI

[ADR-0038](../adr/0038-toolchain-and-ci.md) による。

- `rust-toolchain.toml` で stable の特定版に固定する。MSRV はその版とし、定期的に更新する。edition は 2024。
- rustfmt と clippy を必須とし、警告をエラーとして扱う。
- CI は GitHub Actions。macOS runner を主とし、Linux はソフトウェア実装の Vulkan で互換経路を検証する。
- CI では値とレイアウトの意味的比較を必須とする。GPU 画素の golden 比較は固定環境（参照機）で実行する。
