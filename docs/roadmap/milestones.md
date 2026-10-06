# マイルストーン

状態: M0 は完了。M1 は全 14 タスクが完了（2026-10-03。統合点は `scripts/demo_cli_m1.py`、[CLI-001 の検証](../testing/cli-001.md)）。M2 は P0 全 10 タスクが完了（2026-10-04。統合点は `scripts/demo_integration_m2.py`、[INTEGRATION-001 の検証](../testing/integration-001.md)）。共有編集・検査 API、CompositionClip、日本語テンプレート、基本音声と ProRes / PCM24 書き出し、blur / shadow、MCP stdio、固定 snapshot の独立 worker と 4K 画像連番を実装した。M2 の STORE-003（P2）は `in_progress` であり、M2 全 11 タスクの完了ではない。M1 で見送った範囲（VEC-004 / VEC-005 / STORE-003 / QA-004 / CACHE-003）と下記の M2 延期範囲は後続タスクに記録した。M3 は主要コアと `apps/macos` の4ページ・実時間音声・横断検証を統合済みで、全 25 タスクの受け入れを完了した。現在の完了判定は `backlog.json` と各検証文書を参照する。M4 の全 12 タスクも受け入れを完了した（2026-10-06。[統合受け入れ記録](../testing/m4-acceptance.md)）。M5 は SIM-001・EXPR-002・AUDIO-005 を含む12件を受け入れ済み。NAME-001 は名称確保の所有者判断が保留のため進行中で、全13件の完了ではない。M6 は未着手。ここで実装済みと明記した範囲以外の API・CLI・スキーマは提案として扱う。タスクの詳細は [backlog](../backlog/BACKLOG.md)。

| 段階 | 成果物 | 主な完了条件 | タスク数 |
|---|---|---|---:|
| M0 | 基盤契約・テスト素材・CI・技術スパイク | 有理数時刻 / ID / Property / 色 / alpha 規約、ツールチェーンと CI、2D title から GPU 出力の最短経路（macOS） | 6 |
| M1 | Headless 2D Motion Core | Shape / Text / Group / Null、キーフレーム、任意時刻レンダー、画像連番 | 14 |
| M2 | NLE 統合・CLI / MCP | CompositionClip、日本語 title、基本音声、基本エフェクト、固定 snapshot、計画 / 適用、書き出し、縦断デモ第 1 段階 | 11 |
| M3 | 実用的な Motion Authoring | macOS ネイティブ GUI（canvas / curve editor、編集・テンプレート・書き出しページ）、テンプレート拡張、基本式、responsive layout、リアルタイム再生、M2 の後続機能、縦断デモ第 2 段階 | 25 |
| M4 | 高品質・高解像度 | サブフレームブラー、temporal cache、8K / HDR 品質、GPU 経路診断、Composition の Media ノード、MCP 新版・GUI の安全モード排他・Windows FFmpeg runtime | 12 |
| M5 | 高度な 2D Motion・未割当編集機能 | Repeater、path 演出、音声連動、ルビ・縦書き、Simulation、式構文、Matte、macOS編集UI、名称・音声採否と経路整理 | 13 |
| M6 | 拡張・他OS・製品配布 | 2.5D、互換アダプター、プラグイン、分散、Windows/Linux GUI、各OSの製品packageとresident保証の拡張 | 12 |

## 方針

- M0 で GPU interop の困難さを確認するが、zero-copy の完全達成を M1 の CPU 検証版まで阻害する必須条件にはしない。
- M1 / M2 は互換経路でも実装を進め、転送コストを明示する。GPU 経路の保証はプラットフォーム / 形式ごとに昇格する。
- macOS (Apple Silicon) を先行する。Windows / Linux は互換経路で CI を通し、M4（GPU-003）以降に保証経路を昇格する。
- M0〜M2 は GUI なしで進める。GUI は M3 で macOS から着手する。
- 各マイルストーンの完了は、属するタスクの受け入れ条件がすべて確認できたことで判定する。

## マイルストーンごとの統合点

| 段階 | 統合の確認 |
|---|---|
| M1 | CLI-001: GUI なしで Shape と日本語 Text のアニメーション連番を生成 |
| M2 | INTEGRATION-001: [縦断デモ第 1 段階](vertical-slice.md) |
| M3 | INTEGRATION-002: [縦断デモ第 2 段階](vertical-slice.md)、QA-002: GUI / CLI / MCP 同等性 |
| M4 | PERF-001: 参照シーンの benchmark |

## 2026-10-06 の未割当範囲の整理（PR #9監査時点の履歴）

[現在の実装範囲と残件](implementation-status.md) に、確認済み・未実装・未検証・保証外を対応付けた。NAME-001、EXPR-002/003、AUDIO-005、MATTE-001、FRAMEBRIDGE-001、GUI-007、INSPECT-002をM5、GUI-005/006、RELEASE-002〜004、GPU-004〜006をM6のplannedへ追加した。追加時は全16件がplannedであった。その後の着手・受け入れ状況は冒頭と正本のbacklog.jsonを参照する。配置は作業計画であり、納期・未決事項の採否を確定するものではない。M3/M4の受け入れ範囲と完了は維持する。

## M2 の延期範囲と後続タスク

以下はM2 closeout時に整理した延期範囲の履歴であり、現在もすべて未完了という意味ではない。M3/M4配置の項目は各受け入れ範囲で完了し、AAC/Opusや製品署名など範囲外の残件は上記の新規タスクへ分離した。M2 の P0 完了は以下の機能や検証の完了を意味しない。既存タスクの受け入れ条件を補い、未割当の範囲は M3 の `planned` タスクへ追加した。RECOVERY-001、GPU-003、COLOR-001、CACHE-003、PERF-001 は既存の M4 配置を維持する。ADR の決定は変更していない。

| 後続タスク | 延期範囲 | 根拠 |
|---|---|---|
| NLE-002（新規、M3 / P1） | 動画 Asset / Generator Clip、clip effects、transition / ripple / リンク連動編集 | [ADR-0051](../adr/0051-nle-placement-and-retime.md)、[NLE-001](../testing/nle-001.md)、[01 データモデル](../architecture/01-data-model.md) |
| AUDIO-003（新規、M3 / P1） | Sequence audio track の A/V mux、clip volume、音量アニメーション、Composition の再帰音声 | [ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)、[ADR-0051](../adr/0051-nle-placement-and-retime.md)、[基本音声](../architecture/audio-000.md)、[08 API](../architecture/08-api-cli-mcp.md) |
| AUDIO-004（新規、M3 / P1） | retimed audio、audio effects、Generator 音声 | [ADR-0051](../adr/0051-nle-placement-and-retime.md)、[NLE-001](../testing/nle-001.md) |
| MEDIA-002（新規、M3 / P1） | AV1 / H.264 / HEVC の movie job profile、AAC / ALAC / AV1 配信用音声の契約 | [ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)、[ADR-0050](../adr/0050-fixed-job-execution-and-publication.md)、[JOB-001](../testing/job-001.md) |
| RENDER-003（新規、M3 / P1） | 長尺・大容量 export、最終面の streaming、node 別 tile allocation と halo 予算 | [ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)、[ADR-0050](../adr/0050-fixed-job-execution-and-publication.md)、[ADR-0053](../adr/0053-integration-evaluated-queries-and-render-tiles.md)、[FX-001](../testing/fx-001.md) |
| MCP-002（新規、M3 / P1） | HTTP transport、resources / prompts / sampling / MCP task、実行中要求のキャンセル、progress、外部 SDK 検証 | [08 API の MCP 実装範囲](../architecture/08-api-cli-mcp.md)、[MCP-001](../testing/mcp-001.md) |
| JOB-002（新規、M3 / P1） | Windows worker detach・上書き禁止 publication、Linux / Windows 実プロセス検証 | [ADR-0050](../adr/0050-fixed-job-execution-and-publication.md)、[JOB-001](../testing/job-001.md) |
| FX-002（新規、M3 / P2） | 非一様 scale / shear 下の正 sigma の blur / shadow | [FX-001](../testing/fx-001.md) |
| API-002（新規、M3 / P2） | scene 検索 / paging、固定履歴 cursor、CLI NDJSON event stream | [API-001](../testing/api-001.md)、[08 API](../architecture/08-api-cli-mcp.md)。評価済み world_transform は ADR-0053 で実装済みのため重複しない |
| SERVICE-002（新規、M3 / P2） | project.create / import の計画・冪等性、Modifier 編集 | [08 API の実装範囲](../architecture/08-api-cli-mcp.md) |
| RELEASE-001（新規、M3 / P1） | LGPL同梱package全体の再配置・動的リンク・署名検証 | [MEDIA-001](../testing/media-001.md)、[AUDIO-000](../testing/audio-000.md)、[12 プラットフォームと依存](../architecture/12-platform-dependencies.md) |
| TEMPLATE-002 / LAYOUT-001（更新、M3） | hold / loop、variant、MediaSlot / DataTable 入力、版移行、明示した tight-ink 帯追従と bounds の段階 | [TEMPLATE-001](../testing/template-001.md)、[07 テンプレート](../architecture/07-templates.md)、[04 レイアウト](../architecture/04-vector-text-layout.md)、[ADR-0053](../adr/0053-integration-evaluated-queries-and-render-tiles.md)。M2 の帯は wrap_width 基準の layout_bounds を維持 |
| EXPR-001（更新、M3） | Expression 設定と共有 query / render の有界評価 | [API-001](../testing/api-001.md)、[08 API](../architecture/08-api-cli-mcp.md) |
| AUDIO-002（更新、M3） | GUI 実時間再生・A/V 同期の seek / 停止 / 再開検証 | [AUDIO-000](../testing/audio-000.md)、[基本音声](../architecture/audio-000.md) |
| RECOVERY-001（更新、M4） | job.resume、完了済み区間の扱い、SIGKILL 後の一時出力回収、rename–DB commit 窓の成果物照合 | [ADR-0050](../adr/0050-fixed-job-execution-and-publication.md)、[14 ジョブ](../architecture/14-jobs.md)、[05 レンダラー](../architecture/05-render-gpu.md) |
| GPU-003（更新、M4） | hardware decode / GPU resident media-render 接続と形式別保証 | [ADR-0048](../adr/0048-media-native-build-and-asset-verification.md)、[MEDIA-001](../testing/media-001.md)、[AUDIO-000](../testing/audio-000.md) |
| COLOR-001（更新、M4） | source 色変換 / 10-bit PQ・HLG 保持、HDR job profile と出力検証 | [ADR-0048](../adr/0048-media-native-build-and-asset-verification.md)、[ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)、[ADR-0050](../adr/0050-fixed-job-execution-and-publication.md) |
| CACHE-003（更新、M4） | GPU texture cache / surface pool、厳密 cache の backend / driver fingerprint | [FX-001](../testing/fx-001.md)、[05 レンダラー](../architecture/05-render-gpu.md) |
| PERF-001（更新、M4） | GPU 二重描画・転送統計の集約、exact seek の効率化、実作品の保存・復元性能と snapshot 再検討 | [05 レンダラー](../architecture/05-render-gpu.md)、[ADR-0048](../adr/0048-media-native-build-and-asset-verification.md)、[ADR-0052](../adr/0052-snapshot-policy-evaluation.md) |
| STORE-003（更新、M2 / P2、in_progress） | Dropbox / ネットワーク FS の安全モード・PROJECT_LOCKED | [STORE-003](../testing/store-003.md)。適応的 snapshot の既定不採用、iCloud Drive、Linux/Windowsの競合・強制終了回復CIは確認済み。実Dropbox管理フォルダ・実network mountが未確認 |

INTEGRATION-001 の host 記録は Apple M1 / macOS / Metal、clean revision `31ea36f` の 4K image_sequence（2 frames / 55 checks）。通常 24 fps 動画・音声 mux・tight-ink 追従・streaming の検証とは扱わない。M2 closeout ではこの既存記録を参照し、実機コマンドを再実行していない。
