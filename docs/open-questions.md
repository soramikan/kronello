# 未決事項

決まっていない論点の一覧。解決したら ADR を追加し、ここから項目を削除して「解決済み」に ADR へのリンクを残す。
「決定時期」は、遅くともそのタスクの着手前に決める必要がある目安。

番号は欠番を詰めない。2026-10-06 に重複を整理した: 過去の「OQ-19 イベントと逆操作情報の保持期間」はその番号を維持する。「OQ-19 圧縮音声 AAC と配信向け音声の採用」として重複掲載していた未決事項は、未使用の OQ-21 へ移した。圧縮音声を指す旧 OQ-19 の参照は OQ-21 として読む。保持期間の過去の決定・参照は変更しない。

## 未決

### OQ-02 商標の確認と名前の確保

名称は Kronello に決定した（[ADR-0042](adr/0042-naming-kronello.md)、経緯は [naming.md](naming.md)）。残っているのは次の 2 点。

- 商標データベース（J-PlatPat、USPTO、EUIPO）の照会。2026-10-06にJ-PlatPat・USPTO・EUIPOの指定条件による限定検索を記録した。類似名の網羅調査や所有者の採用判断は未完了（[調査記録](testing/name-001.md)）。
- crates.io の `kronello` と主要な `kronello-*`、ドメインの確保。2026-10-06に公式APIで21 crate名とkronello.comの登録レコードを照会したが、確保・購入は実施していない。照会結果は所有権や取得時の空き状況を保証しない。

決定時期: 公開前としていたが、2026-10-06 時点でリポジトリは public であり、この判断時期は経過した。上記の限定検索・登録照会は実施済みだが、網羅的な確認・名前の確保・所有者の採用判断は未完了である。未決のまま [NAME-001](backlog/backlog.json)（in_progress）で追跡する。新しい期限や取得の承認をここでは決めない。

## タスク内で設計する事項

方針は決定済みで、細部を担当タスクの中で決めるもの。未決事項としては扱わない。

| 事項 | タスク |
|---|---|
| 高頻度経路での FFI の JSON 直列化コストの計測と対策 | FFI-001 |
| 外部変更が来たときの GUI 上の扱い（選択中のオブジェクトの消失など）。提案は [エラーと競合の状態](design-system/screens/states.md) | GUI-001 |
| UI フォントの同梱形態（可変フォントかウェイト別か、サブセット）と、デザイントークンを Swift の定数へ写す方法 | GUI-001 |
| Windows でのプレビュー面の受け渡し | [GUI-005](backlog/backlog.json)（planned）の着手時 |
| Linux でのプレビュー面の受け渡し | [GUI-006](backlog/backlog.json)（planned）の着手時 |

## 解決済み

| 項目 | 決定 |
|---|---|
| OQ-14 性能目標の確定 | [ADR-0093](adr/0093-m4-reference-preview-performance-target.md) |
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
| OQ-17 式言語の構文 | [ADR-0105](adr/0105-human-readable-expression-syntax.md) |
| OQ-19 イベントと逆操作情報の保持期間 | [ADR-0030](adr/0030-history-retention.md) |
| OQ-20 ジョブ記録の保持期間 | [ADR-0034](adr/0034-job-retention.md) |
| OQ-21 圧縮音声 AAC と配信向け音声の採用 | [ADR-0106](adr/0106-versioned-compressed-delivery-audio.md) |
| テスト素材の同梱サイズの閾値と大きい素材の取得元（QA-001 で設計） | [fixture と解析的 golden scene](testing/fixtures.md) |
| OQ-02 のうち名称の衝突 | [ADR-0042](adr/0042-naming-kronello.md) |
| 保存場所の判定・上書き手段（STORE-001） | [ADR-0046](adr/0046-store-format-and-location-policy.md) |
| 履歴警告の閾値（STORE-001） | [ADR-0046](adr/0046-store-format-and-location-policy.md) |
| 素材の hash 照合頻度・同梱 FFmpeg の版と configure（MEDIA-001） | [ADR-0048](adr/0048-media-native-build-and-asset-verification.md) |
| 音声コーデックの選定（AUDIO-000） | [ADR-0049](adr/0049-audio-bus-timing-and-codec.md) |
| 中断ジョブの完了区間再利用と公開境界の照合（RECOVERY-001） | [ADR-0087](adr/0087-fixed-job-resume-and-reconciliation.md) |
| HDR の表示変換と明示 SDR 出力の演算（COLOR-001） | [ADR-0086](adr/0086-rec2100-native-precision-and-fixed-hdr-output.md) |
