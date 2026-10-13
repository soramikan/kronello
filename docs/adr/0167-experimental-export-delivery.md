# ADR-0167: GPU encoder surfaceと直接packet muxを明示した試作機能へ隔離する

- 状態: 採用（試作の境界と既定経路への採用判断）
- 日付: 2026-10-13
- 対象: PERF-014
- 既存ADRの置換: なし。公開exportの色・alpha・time・publication・capabilitiesを変更しない。

## 問題

先行改善後もRGBA16FのCPU読戻し/色変換と圧縮video intermediateの保存/再読込が残る。GPU専用経路や直接muxを全profileへ適用すると、色丸め、surface寿命、codec/container metadataを変える可能性がある。

## 決定

1. `kronello-framebridge/encoder-prototypes`を明示したmacOS buildだけに、Metal RGBA16F→BT.709 BGRA8 IOSurface→hardware VideoToolbox H.264の比較入口を含める。CPU比較も同じMetal fixtureと同じencoder設定を使う。これは独立したnative backend試作であり、wgpuのproduction rendererや公開exportの入力経路を置換しない。
2. 対象は有限のSDR linear Rec.709 premultiplied、opaque black matte、24fps、正の偶数寸法・最大4K、最大2400frames。CPUとGPUは同じ65,536件のhalf→f32 BT.709 transfer/rounding tableを使う。NaN/範囲外は成功前に拒否する。GPU完了とVT callback完了の両方を待って一枚のsurfaceを再利用し、失敗でもsessionをinvalidateしてから所有物を解放する。
3. 比較の出力はvideo-only H.264/MOVで、全CMSampleBufferと保存後の全packetのPTS/DTS/durationを1/24の有理数と照合する。初期Annex Bでcontainer時刻を保持できなかった結果も記録する。YUV420/video range/BT.709を独立decodeで確認する。VT private SEIを含む圧縮bytes差はそのまま報告し、decoded一致へ読み替えない。
4. `kronello-media/export-prototypes`の直接mux対象は、既存SDR ProRes/stereo PCM24/48k/MOV、zero-origin・整数audio sample span、生成silence・chaptersなしの比較とする。最終probe/metadata/時刻と全file bytesを比較する。対象外の入力を受け取って無視するAPIを作らない。
5. 両試作ともprivate stageとno-clobber publicationを使う。既定の公開export/worker/capabilitiesには採用しない。GPU試作はwgpu renderer/tile/temporal/HDR/他matteとの接続、直接muxは実音声・nonempty chapters・priming codec・複数leg・実workerの統合検証が必要である。今回の速度は測定fixture内の全体時間であり、全作品の速度保証ではない。

## 根拠と影響

[PERF-014](../testing/perf-014.md)に固定binary/fixture、交互試行のfirst/warm p50/p95、全decoded画像/音声・色・packet/metadata、surface寿命・取消・異常・既存出力保護の証拠を保存する。GPU専用転送をproduction機能として宣言せず、試作を再実行・レビューできる状態にする。PERF-006〜013の既定経路改善はこの採用保留から独立して利用できる。
