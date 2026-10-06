# ADR-0086: Rec.2100 native 精度と固定 HDR 出力

- 日付: 2026-10-06
- 状態: 採用
- 対象: COLOR-001
- 関連: [ADR-0037](0037-hdr-policy.md)、[ADR-0044](0044-color-and-alpha-contracts.md)、[ADR-0080](0080-root-temporal-integration.md)、[ADR-0085](0085-composition-media-and-template-slots.md)

## 決定

`RenderProfile.hdr` は `HdrSettings { transfer: pq | hlg }` の optional 固定入力とする。HDR は `LinearRec2020` と `SemanticVersions.hdr = 1` を必須とし、legacy snapshot の欠落版を最新で補わない。HDR のない旧 JSON は再保存後も欠落を維持する。working RGB の 1 は **203 cd/m²**、alpha は従来の無次元 premultiplied 契約を維持する。

PQ は ST 2084 の絶対輝度を 203 で除して working RGB に変換する。HLG は Rec.2100 の inverse OETF と RGB luminance による OOTF を組み合わせる。版 1 の HLG は D65 Rec.2020、1000 cd/m² peak、system gamma 1.2 を固定する。出力では逆変換を行う。75% HLG は約 203 cd/m² となる。根拠は [BT.2100](https://www.itu.int/dms_pubrec/itu-r/rec/bt/R-REC-BT.2100-2-201807-S%21%21PDF-E.pdf) と [BT.2408](https://www.itu.int/dms_pub/itu-r/opb/rep/R-REP-BT.2408-8-2024-PDF-E.pdf) とする。

native source plane の `yuv420p10le` / `yuv422p10le` / `yuv444p10le`、`bt2020` / `bt2020nc` / `tv|pc`、`smpte2084|arib-std-b67` を検証して受け入れる。FFmpeg shim は BT.2020 matrix/range 変換のみを RGBA64LE へ行い、Rust が transfer と OOTF を処理する。RGBA8、SDR へのタグ変更、暗黙 tone map を経由しない。HDR profile 内の SDR video も RGBA64 と BT.709/sRGB transfer を使い、native 10-bit 素材を混在させても HDR 作業への入口で暗黙 8-bit 化しない。asset の hash と locked metadata を実際の frame と照合する。別 native format、欠落 HDR tags、色契約の不一致は型付きエラーとする。

CPU と明示的な CPU decode / GPU upload backend は同じ HDR working pixels を使う。`require_gpu_resident` の現 native decoder は HDR を未対応として型付きエラーにする。HDR を SDR として継続しない。

## 出力 profile

同期 `render.export` と `render.submit` に `pro_res_hdr_mov` profile version 1 を追加する。`transfer` に PQ/HLG を指定し、固定 render profile と一致させる。codec は LGPL runtime の `prores_ks`、HQ profile 3、`yuv422p10le`、MOV、48 kHz stereo PCM24 とする。RGBA64LE の transfer encoded input を native encoder へ渡す。出力 color tags は `bt2020` / 対応 transfer / `bt2020nc` / `tv`。probe は native pixel format と 4 色 tags を返し、最終 publication 前に HDR profile と一致することを検証する。別 codec や encoder の暗黙 fallback はない。

working negative / transfer peak 超過値は numeric artifact では保持する。HDR movie の表現域外（HLG inverse OOTF 後の signal code [0,1] 超過を含む）はエラーにし、勝手な clamp、gamut map、tone map を行わない。

SDR display artifact は compositing 後に Rec.2020→Rec.709、負成分の表示域 clamp、各 channel の Reinhard `v/(1+v)`、sRGB encoding を適用する。linear artifact は変えない。native GUI の single DAG preview はこの display 処理を持たないため HDR を型付き未対応とし、`render.frame` の display artifact を利用する。

SDR movie 変換は別の `pro_res_sdr_from_hdr_mov` version 1 で明示する。この profile だけが opaque background 合成後の HDR を上記 Reinhard SDR linear に変換し、BT.709 SDR encoder へ渡す。legacy SDR profile は HDR render を拒否する。

## 8K と資源予算

`OutputRegion` は UHD 8K の 33,177,600 pixels を上限にする。CPU の linear/display 全フレーム保持は引き続き 512 MiB で拒否する。8K は `render_frame_tiles` と streaming movie export の tile ごとの backpressure を利用する。text、matte、blur による glow の実行域と halo の予算は従来の DAG 検証で制限する。

## 検証

[COLOR-001 検証記録](../testing/color-001.md) に native 10-bit 階調、PQ/HLG roundtrip、CPU/GPU、同期・固定 worker、8K の結果を記録する。
