# ADR-0133: 配信拡張（チャプター・マルチ出力・コーデック profile）

- 状態: 採用
- 日付: 2026-10-08

## 背景

MEDIA-004 はチャプターマーカー・マルチ出力・DNxHD/GIF/MP3 等の
追加コーデック profile を要求する。書き出し要求の構造と、
どの profile を追加コーデックとして保証するかを決める。

## 決定

- チャプターは Sequence の `markers`（role=chapter の Marker）から
  コンテナチャプターへ写す。対象は chapter を保持できる muxer
  （mov/mp4/mkv）に限定し、保持できない出力形式では request 側で
  明示しない限り型付き警告を出す。marker の時刻は正本の有理数から
  コンテナの timebase へ変換し、開始時刻・タイトル（色・注記は
  コンテナに保持できないため落とす）を写す。
- マルチ出力は `export.render` の JobRequest に `outputs:
  Vec<OutputSpec>` を追加する。レンダーは 1 回、出力ごとに
  encoder/muxer を並列 tee する。`export.batch`（複数 job の逐次
  実行、ADR-0130）とは別物であり、frame 共有による再レンダー回避が
  目的である。出力ごとの失敗は job 全体の失敗とし、部分的な成功を
  黙って報告しない。既存の単一出力は `outputs` 長 1 の退化形。
- 追加コーデック profile は次の集合とし、それぞれ versioned profile
  として encoder 存在を実行時検証し、無ければ
  `UNSUPPORTED_FEATURE` で拒否する:
  - DNxHD/DNxHR（FFmpeg native `dnxhd`、mov/mxf）
  - GIF（palettegen/paletteuse の二段、native gif）
  - MP3（vendored LAME。LGPL-2.0 で配布ポリシー適合。
    `native-dependencies.json` に lame 3.100 を追加済み）
  - FLAC（native flac。ロスレス音声 profile）
- profile の定義は ADR-0130 の ExportPreset と同じ versioned
  モデルに乗せ、CLI/MCP/GUI で同一の名前解決を使う。
