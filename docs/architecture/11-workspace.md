# 11 ワークスペースと実装責務

状態: Cargo workspace を整備済み。M0 の GPU / FrameBridge スパイク、M1 の store / animation / eval / vector / text / render / service / cli と、M2 の media / audio / template / mcp / jobs の各 crate を実装した。M2 の P0 全 10 タスクは完了し、共有編集・検査 API、CompositionClip のマルチトラック配置、公開入力・保護時間区間付き日本語テンプレート、基本音声と ProRes / PCM24 書き出し、blur / shadow、MCP stdio、固定 snapshot の独立 worker、4K 縦断デモを接続した（実装範囲と検証は下記・各タスクの記録）。延期範囲は[後続タスク](../roadmap/milestones.md#m2-の延期範囲と後続タスク)に記録し、STORE-003 は実環境検証の残件により `in_progress`。M3 以降と `apps/` は未着手。以下の全体構成は引き続き構成案であり、記録した範囲外の API・crate の機能を実装済みとは扱わない。

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
  kronello-jobs/           # detached workers, execution state, leases, publication
  kronello-ffi/            # FFI boundary for native GUI apps
apps/
  macos/                     # Swift (SwiftUI / AppKit) desktop app
  windows/                   # 将来
  linux/                     # 将来
```

v0.2 仕様からの変更: 接頭辞 `ved-` → `kronello-`。`ved-desktop` を廃し、`kronello-ffi` と `apps/` に置き換えた（[10 デスクトップ GUI](10-desktop-gui.md)）。

## 依存の向き

処理の流れは「意味的文書 → 値・組版・Scene IR → Render DAG / 実行計画 → backend」。コード依存は以下の `利用側 → 型・契約の提供側` として読む（[ADR-0043](../adr/0043-semantic-dependencies-and-units.md)「論理モジュールとコード依存」）。

```text
model -> time
animation / expr / text / vector -> model / time
layout -> model / time / text
scene -> model / time / animation / expr / layout / text / vector
template / store -> model / time
render -> scene / model / time (+ evaluation / layout / geometry)
backend (gpu / media / audio) -> render contracts / model / time
framebridge -> gpu / media adapters
service -> store / template / scene / render / backend / framebridge
cli / mcp / ffi -> service
```

- Timeline の文書型（Sequence / Clip）は model、Composition のシーン処理は scene、Property の評価は animation / expr、配置と Scene IR を共通 DAG へまとめる責務は render に置く。専用 Timeline crate を追加する決定ではない。
- render は具象 backend を import せず、非依存の契約を定義し、実装を service / worker から受け取る。Property / Layout の依存 DAG は意味的入力値で接続し、crate の循環依存を作らない。
- store は文書を供給し、service が評価エンジンを呼ぶ。UI は共通 API 経由で利用し、評価エンジンは store / service / UI へ逆依存しない。
- GPU texture、AVFrame、SQLite connection、Tokio runtime の型を model に漏らさない。
- FFmpeg や wgpu の API 差分はアダプターで吸収し、`Cargo.lock` と native dependencies manifest を固定する。
- cli / mcp / ffi は service の薄いアダプターとし、編集の意味を持たない。

## ツールチェーンと CI

[ADR-0038](../adr/0038-toolchain-and-ci.md) による。

- `rust-toolchain.toml` で Rust stable 1.95.0 に固定済み。MSRV は 1.95、edition は 2024。更新時は toolchain と workspace の `rust-version` を同時に更新する。
- ルート `Cargo.toml` は virtual workspace（`members = ["crates/*"]`、resolver 3）。共通 package 設定は version 0.0.0、`MIT OR Apache-2.0`、`publish = false`。各 crate は共通設定と lint を継承し、`Cargo.lock` を管理する。
- rustfmt と clippy を必須とし、警告をエラーとして扱う。
- `.github/workflows/ci.yml` は main への push と pull request を対象とし、`macos-latest`（Apple Silicon）と `ubuntu-latest`（Mesa lavapipe）の両方で fmt / clippy / workspace test / backlog check を実行する。checkout と cache の action は commit SHA に固定し、Rust は `rustup show` で toolchain ファイルから導入する。
- Linux は `VK_DRIVER_FILES` で lavapipe の ICD を選び、`WGPU_BACKEND=vulkan` を設定する。`vulkaninfo --summary` で CPU device とソフトウェア driver を確認する。GPU-001 は M1 / Metal の実機で検証済み。Linux は cross-check のみで、CI / lavapipe の実行成功は未確認。
- CI では値とレイアウトの意味的比較を通常の `cargo test` に含めて必須とする。GPU-001 の色・alpha・座標・転送経路も通常テストに含む。GPU 画素の golden harness は実装済みで CI の必須ジョブにせず、[固定環境の比較手順](../testing/golden-comparison.md) に従う。M4 参照機の基準は未登録。

## GPU-001 で追加した crate

| crate | 現在の実装範囲 | 依存・境界 |
|---|---|---|
| `kronello-gpu` | wgpu 30.0.1 / pollster 1.0.1、CPU 色参照、矩形・PAM の線形 RGBA16F 合成、degree 回転、設計寸法と出力解像度の分離、最小 isolated root group opacity、readback、転送 counters、ignored golden harness | 純粋モデル層に依存・GPU 型を追加しない。`kronello-testkit` は dev-dependency。製品 Render DAG / texture pool は未実装 |
| `kronello-framebridge` | `PathKind` / `TransferPath`、CPU upload / GPU copy / readback、macOS の同一 Metal device 上での IOSurface BGRA8 取り込み・出力スパイク | gpu と wgpu に依存。native interop と unsafe は macOS module。VideoToolbox session は未実装 |

共通 package 設定は両 crate とも継承する。`kronello-gpu` は共通 lint の `unsafe_code=forbid` を継承する。`kronello-framebridge` だけは native interop のため `unsafe_code=allow` とし、Cargo が lint テーブルの部分上書きと workspace 継承を併用できないため、workspace の他の lint 設定を crate に明記する。純粋層の forbid は変更していない。

M1 の通常テストは GPU 13 / FrameBridge 3 が成功。Linux lavapipe、VideoToolbox、M4 参照機は未検証。計測値・転送の数え方・後続タスクの境界は [M0 GPU スパイク報告](../testing/gpu-spike-m0.md) を参照。
