# 未決事項

決まっていない論点の一覧。解決したら ADR を追加し、ここから項目を削除して「解決済み」に ADR へのリンクを残す。
「決定時期」は、遅くともそのタスクの着手前に決める必要がある目安。

番号は欠番を詰めない。

## 未決

### OQ-02 商標の確認と名前の確保

名称は Kronello に決定した（[ADR-0042](adr/0042-naming-kronello.md)、経緯は [naming.md](naming.md)）。残っているのは次の 2 点。

- 商標データベース（J-PlatPat、USPTO、EUIPO）の照会。これまでの調査では一度も照会していない。
- crates.io の `kronello` と主要な `kronello-*`、ドメイン（調査時点で `kronello.com` は未登録）の確保。いずれも先着順。

決定時期: 公開前。名前の確保は早いほどよい。

### OQ-14 性能目標の確定

[13 品質と性能](architecture/13-quality-performance.md) の数値は暫定目標として採用した。合否基準としての確定は、参照シーンを第一の基準機（M4 Mac mini 32GB）で実測した後に行う。
決定時期: PERF-001（M4）。

### OQ-17 式言語の構文

方針は決定した（[ADR-0040](adr/0040-expression-language-policy.md)）: 正本は AST、人間向けには中置演算と関数呼び出しだけの式言語、JavaScript 互換にしない。構文の詳細（演算子、リテラル、Property 参照の書き方、エラー表示）は未定。
決定時期: EXPR-001 の後、GUI で式入力が必要になる M3。

### OQ-19 圧縮音声 AAC と配信向け音声の採用

MEDIA-002 では AV1 / H.264 / HEVC の MOV profile の音声を ALAC に限り、AAC は `UNSUPPORTED_FEATURE` とした（[ADR-0068](adr/0068-versioned-delivery-movie-profiles.md)）。残っているのは次の点。

- AAC-LC を採用するかどうか。配布と特許の確認、FFmpeg 内蔵 AAC encoder の品質評価、priming / padding と終端 sample の扱いの実測が済んでいない。
- Web 配信向けの AV1 の音声（MP4 / WebM の Opus）。LGPL 構成の FFmpeg に libopus を追加する必要がある。

決定時期: Web 配信向けの書き出しを保証する前（M4 以降）。

## タスク内で設計する事項

方針は決定済みで、細部を担当タスクの中で決めるもの。未決事項としては扱わない。

| 事項 | タスク |
|---|---|
| 高頻度経路での FFI の JSON 直列化コストの計測と対策 | FFI-001 |
| 外部変更が来たときの GUI 上の扱い（選択中のオブジェクトの消失など）。提案は [エラーと競合の状態](design-system/screens/states.md) | GUI-001 |
| UI フォントの同梱形態（可変フォントかウェイト別か、サブセット）と、デザイントークンを Swift の定数へ写す方法 | GUI-001 |
| 中断したジョブの再開で、完了済み区間をどこまで再利用できるか | RECOVERY-001 |
| HDR のトーンマッピングと色域圧縮の演算の詳細 | COLOR-001 |
| Windows / Linux でのプレビュー面の受け渡し | 各 GUI の着手時（タスク未作成） |

## 解決済み

| 項目 | 決定 |
|---|---|
| OQ-01 GUI / CLI / MCP の優先順位の判断基準 | [ADR-0041](adr/0041-core-api-gui-order.md) |
| OQ-03 レンダージョブの実行主体 | [ADR-0025](adr/0025-detached-render-workers.md) |
| OQ-04 複数プロセス編集時の Undo の意味 | [ADR-0026](adr/0026-selective-undo.md) |
| OQ-05 WAL と単一ファイル性 | [ADR-0027](adr/0027-wal-single-file-on-close.md) |
| OQ-06 素材参照の再リンクと受け渡し形式 | [ADR-0028](adr/0028-asset-references-and-relink.md) |
| OQ-07 JSON スナップショットのスキーマ | [ADR-0029](adr/0029-public-json-schema.md) |
| OQ-08 FFI の方式 | [ADR-0031](adr/0031-ffi-c-abi-json.md) |
| OQ-09 Windows / Linux のネイティブフレームワーク | [ADR-0032](adr/0032-windows-winui-linux-gtk.md) |
| OQ-10 GUI の UI 状態の保存先 | [ADR-0033](adr/0033-ui-state-in-user-state-area.md) |
| OQ-11 既定の作業用色空間 | [ADR-0024](adr/0024-working-color-space.md) |
| OQ-12 ハードウェアエンコーダーがない環境での書き出し | [ADR-0035](adr/0035-software-encoders.md) |
| OQ-13 FFmpeg の配布方法 | [ADR-0036](adr/0036-ffmpeg-distribution.md) |
| OQ-15 Rust の版と CI | [ADR-0038](adr/0038-toolchain-and-ci.md) |
| OQ-16 テスト素材とフォントのライセンス | [ADR-0039](adr/0039-test-fixtures.md) |
| OQ-18 HDR の基準白・表示変換・色域マッピング | [ADR-0037](adr/0037-hdr-policy.md) |
| OQ-19 イベントと逆操作情報の保持期間 | [ADR-0030](adr/0030-history-retention.md) |
| OQ-20 ジョブ記録の保持期間 | [ADR-0034](adr/0034-job-retention.md) |
| テスト素材の同梱サイズの閾値と大きい素材の取得元（QA-001 で設計） | [fixture と解析的 golden scene](testing/fixtures.md) |
| OQ-02 のうち名称の衝突 | [ADR-0042](adr/0042-naming-kronello.md) |
| 保存場所の判定・上書き手段（STORE-001） | [ADR-0046](adr/0046-store-format-and-location-policy.md) |
| 履歴警告の閾値（STORE-001） | [ADR-0046](adr/0046-store-format-and-location-policy.md) |
| 素材の hash 照合頻度・同梱 FFmpeg の版と configure（MEDIA-001） | [ADR-0048](adr/0048-media-native-build-and-asset-verification.md) |
| 音声コーデックの選定（AUDIO-000） | [ADR-0049](adr/0049-audio-bus-timing-and-codec.md) |
