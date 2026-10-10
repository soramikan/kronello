# ADR-0142: 楽曲全体を扱う音声解析版2と具体的な診断

- 状態: 採用
- 日付: 2026-10-10
- 対象: AUDIO-012
- 部分置換: [ADR-0096](0096-offline-audio-feature-assets.md) の新規解析に使える版・hop・work上限、および保存イベントの session 指定。版1の契約は維持する。

## 背景

版1の100,000,000 work上限では、241.5秒の楽曲をwindow 2048 / hop 1600 / 3帯域で解析できない。
10分のPCM上限は解析可能な長さの保証ではなかった。映像フレームごとの音声連動に必要な
設定を受理し、拒否時には修正するfieldと制限を示す必要がある。

## 決定

- `AudioAnalysisConfig.version = 2` を明示選択する。版1のhop上限・work上限、保存済み
  DataAssetの意味を変えない。既知版は1と2、未知版は従来どおり保持して実行を拒否する。
- 版2のhopは1..=28,800,000 sample。hop > windowでは窓間の入力を読み飛ばす。
  zero-order holdは次の窓まで続き、解析domainの終端は半開区間のまま。
- workの計算式は `ceil(samples / hop) * window * 2 * (log2(window) + bands + 1)`。
  版2の上限は1,500,000,000。10分・48 kHz・window 2048 / hop 1600 / 8帯域の
  18,000窓は1,474,560,000 workで収まる。版1は100,000,000のまま。
- PCM 28,800,000 frames、解析窓65,536、帯域0..=32、48 kHz stereo、
  window 32..=8192のpower-of-twoという上限は維持する。予算は処理時間の保証ではない。
- RMS・FFT帯域エネルギー・onset・beat・padding・time-map・入力hash・決定性は版1と同じ。
  版1と版2で同一の有効設定を解析すると同一環境の特徴量は完全一致する。
- modelの共通preflightを保存DataAsset・audio producer・serviceで使う。
  BusはPCM確保・準備前、Assetは既存の有界decode後かつstereo変換・FFT・出力確保前に
  正確なsample数で検証する。Assetのdecode自体も既存10分上限のappend前検証で有界とする。
- `AudioAnalysisError` はfield別variantと予算のresource / required / allowedを持つ。
  共有APIのcodeは設定の `INVALID_REQUEST`、予算の `AUDIO_BUDGET_EXCEEDED` を維持し、
  `message`に実値・許容範囲、`details`に機械可読の診断を返す。audioライブラリの設定codeは
  従来どおり `INVALID_AUDIO_INPUT`。
- `audio.analyze` に省略可能な `session_id: UUID` を追加する。指定時は保存イベントに伝播し、
  idempotencyの要求同一性にも含める。省略時は従来の自動生成を維持し、JSONから省略して
  既存receiptのcanonical payloadを変えない。返却型は従来の `ProjectInfo` を維持する。

## 検証

[AUDIO-012](../testing/audio-012.md)。新たなFFTアルゴリズム・描画時decode・自動再解析は導入しない。
