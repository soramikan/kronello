# ADR-0134: 外部モニタ出力とベンダー SDK 境界

- 状態: 採用
- 日付: 2026-10-08

## 背景

IO-001 は SDI/NDI 等のリファレンス外部出力を要求するが、
NDI・Blackmagic DeckLink はいずれもプロプライエタリ SDK で、
ベンダー取得・再配布条件の都合上リポジトリへ同梱できず、実機
検証にはハードウェアが要る。検証可能な範囲と、ベンダー経路を
断ち切らない境界を決める。

## 決定

- `OutputDevice` 抽象を導入し、外部出力経路は全て同じ契約
  （format/colorspace/frame pacing/query+typed error）を通す。
- 実装する経路は次の 2 系統:
  - `ref_monitor`: 専用の全画面出力。macOS は外部ディスプレイ上の
    fullscreen window に CAMetalLayer を置き、出力先ディスプレイの
    色空間（ICC/EDID 由来）を明示して描画する。色精度は
    render→surface の readback が program monitor と同じ変換結果に
    なることで検証する。
  - `syphon`: BSD-2-Clause の Syphon framework を runtime 検出し、
    存在時のみ weak link で Metal texture を publish する。受信側の
    検証は in-repo テストクライアントで roundtrip する。
- SDI/NDI/デッキ制御は vendor SDK adapter の境界のみ定義する。
  SDK・デバイス未検出時は `UNSUPPORTED_FEATURE` の型付き拒否とし、
  静かな no-op や代替出力への暗黙 fallback はしない。
- 外部出力の有効化はユーザー/呼び出し側の明示操作とし、
  プレビューの既定経路を変えない。
