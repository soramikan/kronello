# ADR-0021: 最初の縦断テストを M2 と M3 の 2 段階に分ける

- 状態: 採用
- 日付: 2026-10-02

## 背景

v0.2 仕様 §13 の縦断テストは GUI での閲覧、縦型 variant、背景帯の文字追従、shadow を含むが、バックログでは INTEGRATION-001 が M2、GUI・responsive layout・variant が M3 にあり、エフェクトと基本音声のタスクが存在しなかった。

## 決定

- 第 1 段階（M2、INTEGRATION-001）: CLI / MCP のみ。横型、尺変更、別テキスト、最小限の背景帯追従と overflow 検出、基本 shadow、音声付き書き出し。
- 第 2 段階（M3、INTEGRATION-002）: GUI での閲覧、縦型 variant、完全な responsive layout、外部変更への追従。
- M2 に FX-001（基本エフェクト）と AUDIO-000（基本音声）を追加する。

## 影響

- 全層の統合リスクを M2 の時点で確認できる。
- 背景帯追従は M2 の最小実装（TEMPLATE-001）と M3 の完全な実装（LAYOUT-001）の 2 回に分かれる。

## 関連

- [縦断テスト作品](../roadmap/vertical-slice.md)
