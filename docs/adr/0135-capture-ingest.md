# ADR-0135: キャプチャ/インジェスト経路

- 状態: 採用
- 日付: 2026-10-08

## 背景

FLOW-004 は録画・デッキ取り込みを要求する。デッキ制御は
SDI/NDI と同じプロプライエタリ SDK + ハードウェア問題を抱える。
録画そのものは macOS の ScreenCaptureKit で実現できるが、ライブ
キャプチャは決定的にテストできない。実装する経路と検証境界を
決める。

## 決定

- `capture.start`/`capture.stop`/`capture.status` をジョブ系 API
  として追加し、capture 本体は ADR-0025 準拠の detached worker が
  担う。UI/service プロセス内で ScreenCaptureKit を掴まない。
- 録画は provisional file（capture 専用ディレクトリ）へ書き、
  `capture.stop` で完了ハッシュを確定してから asset 登録する。
  未確定ファイルは作品状態に現れず、worker 死後の orphan は
  起動時照合で型付き報告する。
- デッキ取り込み（DeckLink/RS-422 等）は ADR-0134 と同一の
  vendor SDK adapter 境界とし、SDK/デバイス未検出時は
  `UNSUPPORTED_FEATURE` の型付き拒否。
- 検証は合成ソース（in-repo の frame source adapter）で
  capture→provisional→hash→asset 登録の全経路を通す。実機の
  ScreenCaptureKit 録画は確認手順として記録するが、決定的
  テストの要件にしない。
- capture の出力形式・フレームレート・色タグは request で明示し、
  録画中のドロップ/リタイム補正は行わない（取り込んだまま記録し、
  編集側のタイムマップに任せる）。
