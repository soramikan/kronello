# 最初の縦断テスト作品

全層を貫く最初の統合テスト。v0.2 仕様 §13 の内容を、バックログのマイルストーンと整合するよう 2 段階に分けた（[ADR-0021](../adr/0021-two-stage-vertical-slice.md)）。

## 作品

10 秒の 4K シーケンスに動画を配置し、5 秒の日本語 lower-third を重ねる。

- 角丸背景、二行の日本語、ロゴ
- intro 0.4 秒、outro 0.3 秒
- 基本 shadow
- 移動・opacity のキーフレーム
- 素材動画の音声を含めて書き出す

同じ定義を 8 秒、別テキスト、縦型 variant で再利用する。

## 第 1 段階（M2、INTEGRATION-001）

GUI なし。CLI と MCP だけで完結させる。

- CLI でテンプレートをインスタンス化し、シーケンスへ配置する。
- 同じ定義を 5 秒と 8 秒、別テキストで配置する（横型のみ）。
- MCP で値と layout bounds を検査する。
- 固定スナップショットから 4K で音声付きで書き出す。

受け入れ条件:

1. 文字数変更に背景帯が追従し、overflow は検出される（layout_bounds への単方向参照と `max_lines` による最小実装）。
2. 5 → 8 秒で intro / outro の長さが変わらない。
3. 同じ時刻を順序を変えて要求しても同じ意味的結果になる。
4. 同じテンプレートの別インスタンスの入力（文字 / 色 / 長さ）が混ざらない。
5. CLI と MCP の操作は同じ revision / event へ到達する。
6. 値とレイアウトの比較は厳密、GPU 画素比較は固定環境の基準と許容誤差で行う。
7. 基本 shadow が premultiplied alpha と線形作業空間で正しく合成される。
8. 書き出した映像と音声がサンプル精度で同期している。

## 第 2 段階（M3、INTEGRATION-002）

- 第 1 段階と同じプロジェクトを macOS GUI で開き、CLI / MCP と同じ値・layout bounds・画を表示する。
- 同じテンプレート定義を縦型 variant で再利用し、再レイアウト結果を検証する。
- GUI を開いたまま CLI / MCP から編集し、GUI が外部変更に追従することを確認する。

受け入れ条件:

1. GUI / CLI / MCP の操作は同じ revision / event へ到達する。
2. 縦型 variant で layout / ink / visual bounds が区別され、循環と overflow が診断される。
3. GUI を開いた状態での外部編集が、再読込後に GUI に反映される。古い revision に基づく GUI 操作は競合として扱われる。

## 依存タスク

| 段階 | 直接の依存 |
|---|---|
| 第 1 段階 | NLE-001、TEMPLATE-001、MCP-001、JOB-001、AUDIO-000、FX-001 |
| 第 2 段階 | INTEGRATION-001、TEMPLATE-002、GUI-001 |
