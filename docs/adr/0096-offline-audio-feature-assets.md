# ADR-0096: 不変の事前音声特徴量を版付き DataAsset として保存する

- 状態: 採用
- 日付: 2026-10-06
- 対象: AUDIO-001

## 決定

共有 `audio.analyze` Command は revision を照合した入力を一度解析し、Project の
`audio_analyses` に不変の `AudioAnalysisDataAsset` を保存する。直接素材は AssetId / stream /
content hash、Bus は固定 Project・target・range・evaluator 2 の hash を入力識別子とする。
Bus は解析時点の履歴入力であり、その後の編集に自動追従しない。解析結果自身を入力 hash に含めない。

解析版1は48 kHz stereo、32〜8192 sample の power-of-two rectangular window、1〜window
sample の hop、最後の zero padding を固定する。RMS は両 channel の平均二乗平方根。
帯域は `[low,high)` Hz の one-sided FFT bin energy、両 channel 平均、window² 正規化。
onset は前窓との差の正値。beat は onset >= 0.05 と100 ms refractoryを満たす onset pulse
であり、tempo / 拍子推定とは扱わない。解析work（両channel FFT・全band/bin判定・入力sample）は100,000,000、入力は既存10分上限。
数値結果は同じ実行環境で決定的。異なる platform のsin/cosのbit一致は保証しない。

各特徴量は絶対 rational `sample/48000` timestamp と保存した time-map を持つ。
Expression version2 の `AudioFeature` は固定 AssetId / feature / rational offset を静的列挙し、
半開区間内の zero-order hold をO(1)参照する。範囲外・欠落帯域・欠落dataは型付き評価失敗。
描画経路にPCM、decoder、FFT、filesystemを渡さない。素材hashが変化したdataは最終renderを拒否する。
共有Commandの元要求をreceiptへ保存し、同一キーの再送はrevision照合・decodeより先に同じ応答を返す。異なる要求で同じキーを使うと拒否する。receiptとDataAssetは同一store transactionで確定する。

旧 Expression version1 と省略時意味は維持する。未知の解析内容はopaque保存し実行しない。

AAC / Opus の採用判断 OQ-21 は変更しない。
