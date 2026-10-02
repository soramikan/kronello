# 06 拡張点: Repeater・Simulation・音声・3D

初期リリースでは実装しないが、境界を先に確保する機能。基本音声だけは M2 で実装する。

## Repeater

- 一つの Source ノードと instance transforms / instance properties を保持する。
- 複製数分の編集オブジェクトを必ず生成する方式にしない。個別編集が必要な場合だけ expand を明示する。
- 描画のまとめ方は Blend / Mask / Effect で変わるため、常に単一 draw call になると約束しない。
- 要素 ID と instance seed を固定し、配列の処理順が乱数に影響しないようにする。

## Simulation

通常のアニメーションは `value = f(snapshot, time, instance)`。
状態を要する粒子等だけ `state(k+1) = step(state(k), inputs(k))`。

通常 Property の純粋評価・依存境界は [ADR-0043](../adr/0043-semantic-dependencies-and-units.md) に従う。Simulation の状態・アルゴリズム版は snapshot の `semantic_versions` に固定し、未対応の必要機能は最終レンダーで拒否する（[ADR-0045](../adr/0045-snapshot-compatibility-boundaries.md)）。

- 固定刻み、固定 seed、版付き状態、入力ハッシュ、チェックポイントを使う。
- シーク時は必要な時刻より前の有効な checkpoint から再計算する。
- 逆再生は元の正方向シミュレーション時刻を参照し、数値積分を逆方向に巻き戻さない。
- 同じ環境内の結果一致を検証し、GPU 原子演算等を含む完全なクロスデバイス決定性を別問題として扱う。
- 分散レンダーは未ベイク Simulation を含む区間を自由分割しない。

## 音声

### 基本音声（M2〜M3）

- M2（AUDIO-000）: 素材音声のデコード、48kHz への変換、クリップ音量、Bus へのミックス、音声付き書き出し。サンプル位置は絶対時刻から計算し、映像との同期をサンプル精度で検証する。
- M3（AUDIO-002）: GUI でのリアルタイム再生と A/V 同期。音声コールバックはプロジェクト更新・ディスク読み出し・式評価と別の実行系にする。

### 音声連動（M5）

- RMS、帯域エネルギー、onset / beat 等を事前解析し、タイムスタンプ付き DataAsset として保存する。
- Asset hash、サンプルレート、窓長、ホップ長、解析版、time-map を固定する。
- 毎描画フレームで音声全体を再解析しない。
- ミックス後音声の特徴量を使う場合は対象 Bus と mix snapshot を固定する。
- 声の速度を自動変更せず、テンプレート SE は intro / outro marker へ配置する方式を優先する。

## 2.5D / 3D

- 当初は 2D。次に平面、奥行き、カメラの 2.5D を Scene3D サブグラフで実装する。
- 2D の描画順と 3D の depth / transparency 処理を同一の z 値だけで統一しない。
- 将来の glTF、キャラクター等は明示した 3D 境界から取り込み、外部レンダラーの色・alpha・必要な補助チャンネルを合成できるようにする。
- フル 3D、リグ、物理、パストレーサーを動画編集 MVP の必須条件にしない。

外部レンダーの Color 入力にも [ADR-0044](../adr/0044-color-and-alpha-contracts.md) の色空間・alpha 表現・変換順を適用する。3D の座標軸・変換型の詳細は本段階では固定しない。補助チャンネルや未知ノードを保持できることと、描画できることは区別する。
