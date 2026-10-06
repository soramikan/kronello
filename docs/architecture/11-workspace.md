# 11 ワークスペースと実装責務

状態: 2026-10-06、M0・M1・M2のP0・M3・M4を受け入れ済み。`apps/macos` は実装済みで、Windows / Linux GUIは未実装（GUI-005 / GUI-006）。STORE-003は実Dropbox管理フォルダ・実ネットワークFSの検証が残る。機能の保証範囲は [M4受け入れ](../testing/m4-acceptance.md)、未実装・未検証の対応表は [現在の実装範囲と残件](../roadmap/implementation-status.md) を参照する。

## 現在の配置

以下は現行 `Cargo.toml` と各crateのmanifestに存在する構成。論理上の機能名と独立crateの有無を混同しない。

```text
crates/
  kronello-model/          # document types, IDs, property descriptors, versions
  kronello-time/           # rational time, ranges, TimeMap, sampling
  kronello-animation/      # curves and interpolation
  kronello-eval/           # property evaluation, bounded expression AST
  kronello-text/           # fonts, Japanese horizontal layout, glyph/cluster mappings
  kronello-vector/         # paths, geometry, fill and stroke
  kronello-render/         # scene compilation, layout, DAG, region/time requests, cache
  kronello-gpu/            # wgpu execution, color/alpha, texture cache and pools
  kronello-media/          # FFmpeg runtime, seek, decode/encode
  kronello-framebridge/    # native interop and resident VideoToolbox/Metal decode
  kronello-audio/          # document audio, mixing, stateless effects
  kronello-store/          # SQLite, snapshots, migrations, event journal
  kronello-template/       # inputs, bindings, durations and versions
  kronello-service/        # shared commands, queries, policies and orchestration
  kronello-cli/            # machine-oriented CLI (binary: kronello)
  kronello-mcp/            # stdio/HTTP MCP adapter
  kronello-jobs/           # detached workers, leases, publication and recovery
  kronello-platform/       # OS process and publication primitives
  kronello-ffi/            # native GUI C ABI
  kronello-testkit/        # fixtures, CPU reference and golden support
apps/
  macos/                  # SwiftUI / AppKit application and SwiftPM tests
```

`kronello-expr` / `kronello-scene` / `kronello-layout` は独立crateとして存在しない。式評価は主にeval、scene compilationとlayoutはrender等に実装されている。`apps/windows` / `apps/linux` は将来の配置候補であり、存在するアプリとして列挙しない。高度な音声特徴量はAUDIO-001、縦書き・ルビはTEXT-002の未実装範囲である。

## 依存の向き

意味上の流れは「文書 → 値・組版・Scene IR → Render DAG / 実行計画 → backend」。[ADR-0043](../adr/0043-semantic-dependencies-and-units.md) の論理境界を保ちつつ、実crateの依存は各 `Cargo.toml` を正本とする。主要なproduction依存は次のとおり（全依存を列挙した図ではない）。

```text
model -> time
animation / eval / text / vector -> model / time
render -> eval / vector / text / template / model / time
store / template -> model / time
service -> store / render / media / audio / jobs / model
cli / mcp / ffi -> service
```

- Timelineの文書型はmodel、配置とScene IRをDAGへまとめる責務はrenderに置く。
- renderのproduction依存にgpuはない。テスト用のgpu dev-dependencyと区別する。
- serviceがstoreの文書から評価・描画を呼ぶ。評価エンジンはstore / service / UIへ逆依存しない。
- GPU texture、AVFrame、SQLite connection、Tokio runtimeの型をmodelへ漏らさない。
- CLI / MCP / FFIは同じCommand / Queryを利用し、GUI専用の作品状態を持たない。
- jobsとplatformの責務、Windowsのnative handle隔離は [ADR-0074](../adr/0074-windows-job-workers-and-process-evidence.md) と [ADR-0082](../adr/0082-windows-ffmpeg-runtime.md) に従う。

## FFI とアプリ

`kronello-ffi` は9関数のC ABI、非同期worker、共有要求のdecoder、native previewを持つ。Swiftの公開型はJSON Schemaから生成する。FFI-002では安全モードのstoreをセッション中保持する。純粋層のunsafe禁止は維持し、native interopだけにunsafeを隔離する。

macOSはEdit / Motion / Template / Exportの4ページと実時間音声を実装した。[macOS README](../../apps/macos/README.md) にbuild手順、[M3受け入れ](../testing/m3-acceptance.md) と [FFI-002](../testing/ffi-002.md) にGUI・process検証を記録する。開発アプリのad-hoc署名と製品配布の保証は異なる（RELEASE-002〜004）。

## ツールチェーンと CI

- Rust stable 1.95.0、edition 2024、workspace `rust-version` 1.95、resolver 3。`Cargo.lock` を管理する。
- virtual workspaceは `crates/*`。共通packageはversion 0.0.0、`MIT OR Apache-2.0`、`publish = false`。各crateは共通設定とlintを継承し、native interop層だけ明記した例外を持つ。
- [CI workflow](../../.github/workflows/ci.yml) はmainへのpushとpull requestを対象とし、checkout/cache等のactionをcommit SHAで固定する。
- macOS Apple Silicon / Linux Mesa lavapipeでfmt、warningsを拒否するclippy、workspace tests、CPU統合、MCP SDK、実process・FFI終了、backlog検証を行う。macOSはSwift build/testも実行する。
- Windowsは固定したLGPL runtimeとCLI/MCPをbuildし、実media roundtrip、親終了後のworker、保存層・運用example・processを検証する。workspace全体をmacOS/Linuxと同じコマンドで実行したとは扱わない。
- QA-004のLinux Vulkan / Windows DX12画像比較を別jobで実行する。software adapterの結果をhardware resident保証に置き換えない。Apple Silicon / Metalは採用済み基準に対する実機比較を [golden手順](../testing/golden-comparison.md) で行う。
- [run 37426096876](https://github.com/soramikan/kronello/actions/runs/37426096876) は全5jobs成功。実行環境・件数・checkout・証拠は [M4受け入れ](../testing/m4-acceptance.md) を参照。古い証拠を再アップロードしないよう、cache復元後に検証出力を消去し、実行されたproducerの証拠だけをuploadする。

M0の矩形/PAM/IOSurface技術スパイクの履歴は [スパイク報告](../testing/gpu-spike-m0.md) に残す。現在のGPU DAG・texture cache/pool・VideoToolbox resident decodeの範囲は [05 レンダラー](05-render-gpu.md) と [GPU-003](../testing/gpu-003.md) を参照し、M0当時の未実装一覧を現状と扱わない。
