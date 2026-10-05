# ADR-0077: テンプレート内部の評価値を停止時の読み取り検査として表示する

- 状態: 採用
- 日付: 2026-10-05
- 対象: INTEGRATION-002

## 背景

GUI-001 の Layers / Inspector は現在の Composition の authored node を扱う。
テンプレート内部を編集可能な別作品状態へ展開せず、stage-1 の実配置を
CLI / MCP と同じ時刻・font lock で検査する必要がある。

## 決定

supervisor の承認した範囲として、Motion の CompositionInstance 選択に
「テンプレート内部（読み取り専用）」を追加する。内部 node の識別は
共有 query の `InstancePath + NodeId`、選択と cache は表示状態だけである。
Property 値は `PropertyPresentation` の名称・倍率・小数一桁を使い、
編集 field と keyframe navigator は出さない。layout / ink / visual の
Inspector 座標と Viewer 矩形は同じ root Composition 空間の query bounds を使う。
Viewer は既存 `KRViewerSelection` の青い枠・寸法ラベルを使い、内部矩形には操作 handle を付けない。
選択中のテンプレート配置にも内部検査と紛らわしい操作 handle を重ねない。

一つの EditorModel の Inspector / Viewer は一つの読み取り cache を共有する。
revision / Composition / 選択配置 / 有理数 time が同じ最後の成功結果は再利用する。
停止時の変更は150msの idle 後に一つの expanded `scene.query` を発行する。
node別・frame別の逐次 request を発行しない。SwiftUI task の cancel と generation
検証で古い結果を破棄し、異なる revision の結果を採用しない。
再生中は request を発行せず最後の結果を保持し、`ink-muted` の
「再生中は停止時に更新」を表示する。pause / seek 後に必要な一回の検査を行う。
font・評価・revision の失敗は `ServiceFailure` と `KRErrorLine` で表示する。
activeな内部 node がない時刻も説明付き `EVALUATION_ERROR` とし、空の成功表示にしない。

core API、保存形式、テンプレート edition / variant / duration の意味を変更しない。
variant は ADR-0059 の明示した binding を使い、出力解像度による暗黙選択をしない。

## 検証

同じ stage-1 project と一つの定義に portrait を加える公開 API driver、
CLI / MCP の canonical bytes、GUI の実 FFI session と表示変換、debounce / cancel /
再生中の request 数を検査する。画面・Metal・SwiftPM の合格は別の host 証拠を要求する。
[検証とホスト手順](../testing/integration-002.md) を参照。
