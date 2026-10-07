# ADR-0102: 固定 DataAsset と有界の過去 sampling・連続 noise

- 状態: 採用
- 日付: 2026-10-06
- 関連: EXPR-003、ADR-0058、ADR-0096

## 決定

式の人間向け構文は決めず、正本の postorder AST に意味版3を追加する。
版1の `Noise` と pin省略の意味1、版2の `AudioFeature` を維持する。
既存pin1/2はその能力内の文書を実行でき、版3 ASTを古いpinへ渡すと拒否する。

`PropertySample` は静的なProperty ID・型とScalar `lookback` operandを持つ。
有限・非負の秒を1ns gridへnearest（非負半端は上へ）量子化し、正規有理数へ変換する。
root時刻からlookbackを引き、対象Propertyの通常のinstance時間写像を適用する。
任意のpiecewise時間写像の逆関数は求めない。型・unit・coordinate spaceを従来と同じく
検証する。過去参照も静的依存辺であり、自分自身や相互のpast cycleを拒否する。
現在時刻で供給されたlayout projectionを過去へ再使用できないため、layoutに依存する
対象の過去samplingは明示的に拒否する。不変のinstance入力は同じquery入力として維持する。

`ContinuousNoise` は有限coordinateのfloorに隣接する2つの整数lattice hashを
固定seed・element・instance pathで生成し、quintic `6t^5-15t^4+10t^3` で補間する。
範囲は `[-1,1]`、coordinateは `[-10^9,10^9]`。境界で値と傾きが連続する。
旧Noiseのfloat bit hash・出力を変更しない。

一般参照はinline `ExpressionDataAsset` の型付きtableへ限定する。version1とtableの
canonical JSON tupleのSHA256を保存し、IDと外部locatorをhashへ混ぜない。
64columns、65536rows、保守的1MiB payload以下で、column型とrow shapeを検証する。
`DataAssetCell` のasset ID・column・出力型は静的、rowは動的Scalar operandで
非負整数かつ存在するindexのみ許す。tableを外部pathや最新資産から再読込しない。
AUDIO-001の特徴量参照は別の既存typed DataAssetとして維持する。

nested samplingには親queryと共通の命令・メモリ・sample予算を渡す。sample処理を
別のpublic evaluationとして起動して予算を初期化しない。各式の小さい上限にも
nested処理の増分を課す。版3の依存schedule構築はkey clone・stack・memo storageを
処理前に保守的課金し、table cell clone前に選択payloadを課金する。noiseは2hash分を課金する。
各requested rootの予算は独立で、batch順や過去queryのcacheで成否を変えない。

## 検証

固定有理数時刻・逆順・再試行、動的lookback、境界、欠落/変更hash、static past cycle、
nested予算、lattice連続性、古いpinと版拒否を、純粋evalと共有API・最終CPU renderで検証する。
検証途中の結果と受け入れは `docs/testing/expr-003.md` を正本とする。
