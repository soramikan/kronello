# 未決事項

決まっていない論点の一覧。解決したら ADR を追加し、ここから項目を削除して「解決済み」に ADR へのリンクを残す。
「決定時期」は、遅くともそのタスクの着手前に決める必要がある目安。

番号は欠番を詰めない。

## 未決

### OQ-02 名称の再検討と商標の確認

Cinewright（[ADR-0023](adr/0023-naming-cinewright.md)）を採用した後の追加調査で、同名の事業者が見つかった。米国メイン州でドキュメンタリー映画制作のワークショップを運営する Cinewright（cinewright.com）である。ソフトウェアは提供していないが、映像制作という隣接分野であり、`.com` ドメインは取得できない。このため名称を再検討する。経緯は [naming.md](naming.md)。

新しい名称を決める際は、候補ごとに次を確認する。

- パッケージレジストリ（crates.io、npm、PyPI、Homebrew）と GitHub
- 候補名単独での Web 検索と、主要ドメインの使用状況
- 商標データベース（J-PlatPat、USPTO、EUIPO）。これまでの調査では一度も照会していない

名称が決まったら、crates.io の名前を早めに確保する（先着順）。

決定時期: 公開前。コードの着手前に決めると crate 名の変更が不要になる。

### OQ-14 性能目標の確定

[13 品質と性能](architecture/13-quality-performance.md) の数値は暫定目標として採用した。合否基準としての確定は、参照シーンを第一の基準機（M4 Mac mini 32GB）で実測した後に行う。
決定時期: PERF-001（M4）。

### OQ-17 式言語の構文

方針は決定した（[ADR-0040](adr/0040-expression-language-policy.md)）: 正本は AST、人間向けには中置演算と関数呼び出しだけの式言語、JavaScript 互換にしない。構文の詳細（演算子、リテラル、Property 参照の書き方、エラー表示）は未定。
決定時期: EXPR-001 の後、GUI で式入力が必要になる M3。

## タスク内で設計する事項

方針は決定済みで、細部を担当タスクの中で決めるもの。未決事項としては扱わない。

| 事項 | タスク |
|---|---|
| 保存場所（同期フォルダ等）の判定方法と、誤判定時の上書き手段 | STORE-001 |
| 履歴の大きさを警告する閾値 | STORE-001 |
| 素材の hash 照合の頻度と高速化 | MEDIA-001 |
| 同梱する FFmpeg の版と configure オプション | MEDIA-001 |
| 音声コーデックの選定 | AUDIO-000 |
| テスト素材をリポジトリに含めるサイズの閾値と、大きい素材の取得元 | QA-001 |
| 高頻度経路での FFI の JSON 直列化コストの計測と対策 | FFI-001 |
| 外部変更が来たときの GUI 上の扱い（選択中のオブジェクトの消失など） | GUI-001 |
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
