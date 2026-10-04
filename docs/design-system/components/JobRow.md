# JobRow

書き出し・解析などのジョブ 1 件を表す行で、StatusBar から開く一覧に並べ、状態ごとに次の操作を示す。

## 利用側が渡すもの

- 種類と対象（「書き出し: Teaser_v3.mp4」「解析: Interview_A.mov」）。
- 状態: `running` | `queued` | `done` | `failed` | `cancelled`。
- `running` は進捗（%）と残り時間、`failed` はエラーコードと説明、`done` は所要時間。
- 操作: 中止 / 取り消し（`x`）、表示、設定… など。ジョブは別プロセスで動くため、UI を閉じても続く。

## 見た目

- 最小高 40px、左右 `space-3`、下端 1px `line`。4 列（状態アイコン 14px / 名前 + 詳細 / 状態 / 操作）。
- 名前は `body` の `ink`、詳細は `caption` の `ink-muted`、進捗の数字は mono 11px。
- 状態アイコン: running `loader-circle`（回転）、queued `clock`、done `circle-check`、failed `triangle-alert`、cancelled `circle-x`。failed だけアイコンと詳細を `danger` にし、エラーコードを mono で出す。
- 進捗バーは StatusBar と同じ（地 `line-strong`、進み `ink`）。

## 使い分け

- する: 失敗は理由と次の操作（設定…、再試行）を同じ行に出す。黙って消さない。
- しない: 完了を緑で、失敗を赤だけで示さない。アイコンの形と文言で区別する。
