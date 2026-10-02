# 02 時間の規約

## 基本規約

- 正本の時刻は正規化された有理数。整数演算は checked とし、中間計算は必要に応じて i128 を使う。
- JSON 上の分子・分母は 10 進文字列とし、JavaScript の整数精度の制限を受けない。
- 区間は原則 `[start, end)`。
- フレームレートは 30000/1001 のように正確に保持する。
- 編集用フレームグリッドと評価時刻は別。フレーム間でも Property を評価できる。
- 音声のサンプル位置は絶対時刻から計算し、映像フレームごとの丸め誤差を累積させない。

```json
{"time":{"num":"1","den":"60"}}
```

## 時間階層

```text
Sequence time
 -> clip placement offset
 -> clip TimeMap
 -> Composition local time
 -> nested instance TimeMap
 -> animation local time
 -> media PTS / feature-data time
```

基本写像は `local = source_in + time_map(parent_time - placement_start)`。

TimeMap には後から区分線形、逆再生、ループ、停止、非線形を追加する。初期は線形と区分線形を実装する。
非線形写像は浮動小数点計算や求根を伴うため、完全に有理数だけで解けるとは扱わない。量子化精度・丸め・評価アルゴリズムの版を固定する。

Composition は既定で親の連続時刻で評価する。編集レートが 24fps でも 60fps 出力時に整数の 24fps フレームへ勝手に丸めない。
コマ撮りのように内部レートを保持したい場合だけ、明示的な posterize / hold サンプリングを指定する。
動画素材の保持・補間・オプティカルフローは別の機能であり、ベクターの連続時間評価と混同しない。

## 純粋評価

通常のアニメーションは任意時刻の純粋評価とする（[ADR-0003](../adr/0003-pure-evaluation-at-arbitrary-time.md)）。同じスナップショット・時刻・インスタンスに対する評価結果は、要求順（順方向・逆順・ランダム）に依存しない。状態を必要とする表現は Simulation（[06 拡張点](06-extensions.md)）へ分離する。

## トリムと長さ変更

`clip.trim`、`clip.stretch`、`template_instance.retime` は別操作とする。
尺の変更によって、保護されたイントロ・アウトロを黙って伸縮しない。
音声のリタイム方針も別に宣言する。複雑な非単調 TimeMap に対する音声処理が未対応なら検証で拒否する。
