# 未決事項

決まっていない論点の一覧。解決したら ADR を追加し、ここから項目を削除する。
「決定時期」は、遅くともそのタスクの着手前に決める必要がある目安。

## プロダクト

### OQ-01 GUI / CLI / MCP の優先順位が衝突したときの判断基準

位置づけは「両者同格」とした。工数が足りず、GUI の操作性と自動化 API の完成度のどちらかを先送りする場面での判断基準は未定。
決定時期: M2 完了時（M3 の GUI 着手前）。

### OQ-02 商標の確認と名前の確保

名称は Cinewright に決定した（[ADR-0023](adr/0023-naming-cinewright.md)、[調査](naming.md)）。残っているのは次の 2 点。

- 商標データベース（J-PlatPat、USPTO、EUIPO）の照会。Web 検索とパッケージレジストリでは衝突が見つからなかったが、商標は未確認。
- crates.io の `cinewright` と主要な `cinewright-*`、および関連ドメインの確保。crates.io は先着順。

決定時期: 公開前。

## 保存・同時編集

### OQ-19 イベントと逆操作情報の保持期間

Undo のためにイベントと逆操作情報を `.cinewright` に永続化する（[ADR-0026](adr/0026-selective-undo.md)）。ファイルが際限なく大きくならないよう、保持期間、古い履歴の圧縮や切り捨て、切り捨てた範囲の Undo 不可の扱いを決める。
決定時期: STORE-001。

### OQ-20 ジョブ記録の保持期間

状態 DB とジョブディレクトリ（固定スナップショット、ログ）の保持期間と掃除の方法（[ADR-0025](adr/0025-detached-render-workers.md)）。
決定時期: JOB-001。

### OQ-05 WAL と単一ファイル性

SQLite を WAL モードで開くと、開いている間は `-wal` / `-shm` の付随ファイルができる。クラウド同期フォルダやネットワークファイルシステム上での安全性、閉じるときの checkpoint 方針、開いたままコピーされた場合の扱いを決める。
決定時期: STORE-001。

### OQ-06 素材参照の再リンクと受け渡し形式

素材は外部参照 + content hash とした。相対パス / 絶対パスの保持方針、移動された素材の再リンク手順、素材同梱アーカイブの形式（zip の展開上限を含む）が未定。
決定時期: MEDIA-001。

### OQ-07 JSON スナップショット / 交換形式のスキーマ

JSON は不変スナップショットまたはインポート形式と位置づけたが、スキーマ本体は未定義。
決定時期: STORE-001。

## GUI

### OQ-08 FFI の生成方式

`cinewright-ffi` の実装方式。候補: UniFFI / 手書き C ABI + cbindgen / swift-bridge。Windows（C# / WinRT）と Linux（C / GObject）からも使える必要がある。Command / Query を型付きで公開するか、JSON 直列化で一本化するかも含む。
決定時期: FFI-001。

### OQ-09 Windows / Linux のネイティブフレームワーク

候補は Windows: WinUI 3、Linux: GTK4。macOS 版の完成後に決める。
決定時期: M3 完了後。

### OQ-10 GUI の UI 状態の保存先

選択・pan / zoom・パネル配置などは作品から分離する（GUI-001）。保存先（ユーザーごとの設定領域か、`.cinewright` 内の別テーブルか）は未定。
決定時期: GUI-001。

## レンダー・メディア

### OQ-12 ハードウェアエンコーダーがない環境での書き出し

LGPL 構成では x264 / x265 を同梱しない（[ADR-0018](adr/0018-ffmpeg-lgpl-dynamic-linking.md)）。Linux など OS のエンコーダーが使えない環境で既定とするソフトウェアエンコーダー（SVT-AV1、OpenH264 等）と、その特許・配布条件の扱い。
決定時期: MEDIA-001。

### OQ-13 FFmpeg の配布方法

動的リンクする FFmpeg を、アプリに同梱するか、システムのものを使うか。対応する FFmpeg の版の範囲と、native dependencies manifest の形式。
決定時期: MEDIA-001。

### OQ-14 性能目標の確定

[13 品質と性能](architecture/13-quality-performance.md) の数値は未計測の受け入れ案。参照シーンと参照機での実測後に確定する。
決定時期: PERF-001。

### OQ-18 HDR の基準白・表示変換・色域マッピング

作業用色空間は決定した（[ADR-0024](adr/0024-working-color-space.md)）。HDR の基準白レベル（SDR 素材を HDR 作品へ置くときの明るさ）、プレビューの表示変換とトーンマッピング、出力時の色域マッピングは未定。HDR の表示変換を最終出力へ勝手に焼き込まない、という原則だけが決まっている。
決定時期: COLOR-001（M4）。SDR のみを扱う M0〜M3 には影響しない。

## 開発基盤

### OQ-15 Rust の版と CI

MSRV、edition、lint 設定、CI（macOS runner、GPU を使うテストの実行方法）は未定。
決定時期: M0 のコード着手時。

### OQ-16 テスト素材とフォントのライセンス

golden scene に使う映像・音声・フォントの権利確認と、リポジトリへ含めるか外部取得にするか。
決定時期: QA-001。

### OQ-17 式の人間向け DSL

最初は型付き AST のみ。DSL の構文は未定。
決定時期: EXPR-001 以降。

## 解決済み

| 項目 | 決定 |
|---|---|
| OQ-03 レンダージョブの実行主体 | [ADR-0025](adr/0025-detached-render-workers.md) |
| OQ-04 複数プロセス編集時の Undo の意味 | [ADR-0026](adr/0026-selective-undo.md) |
| OQ-11 既定の作業用色空間 | [ADR-0024](adr/0024-working-color-space.md) |
| OQ-02 のうち名称の衝突 | [ADR-0023](adr/0023-naming-cinewright.md) |
