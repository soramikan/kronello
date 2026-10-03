# fixture 台帳

正本: [manifest.json](manifest.json)。全素材の出典・ライセンス・用途をここにも列挙する。生成データは本プロジェクトの MIT OR Apache-2.0、外部フォントとライセンス本文は OFL-1.1。FFmpeg 本体は同梱しない。

| ID | 保存 | ライセンス | 出典・用途 |
|---|---|---|---|
| `japanese` | bundled | MIT OR Apache-2.0 | scripts/fixtures.py canonical_data (original generated data) — 結合濁点・1 書記素複数 glyph・合字・IVS・emoji・異体字・禁則・ruby・縦書きの入力。TEXT-001 は固定フォントの組版対応と欠落・未対応エラーを確認する。 |
| `timing` | bundled | MIT OR Apache-2.0 | scripts/fixtures.py canonical_data (original generated data) — 有理数の CFR/VFR サンプルと長尺フレーム時刻。num/den は正規化した decimal string。 |
| `alpha` | bundled | MIT OR Apache-2.0 | scripts/fixtures.py canonical_data (original generated data) — straight alpha の透明有色・半透明・不透明・低 alpha。 |
| `linear-hdr` | bundled | MIT OR Apache-2.0 | scripts/fixtures.py canonical_data (original generated data) — 線形 premultiplied Rec.2020。負 RGB・1 超・ゼロ alpha・微小 alpha。 |
| `sine-48k-stereo` | bundled | MIT OR Apache-2.0 | scripts/fixtures.py canonical_data (original generated data) — 48kHz stereo PCM16 の固定 1kHz sine。後半で右チャンネルを有効化。 |
| `noto-license` | bundled | OFL-1.1 | https://raw.githubusercontent.com/notofonts/noto-cjk/523d033d6cb47f4a80c58a35753646f5c3608a78/LICENSE — 取得フォントに付属する原文ライセンス。変更せず保存。 |
| `cfr-24-1` | generated | MIT OR Apache-2.0 | scripts/fixtures.py media_command; FFmpeg testsrc2; rawvideo/NUT — 24/1 fps、16x16、6 frames。各 PTS を有理数で照合。 |
| `cfr-25-1` | generated | MIT OR Apache-2.0 | scripts/fixtures.py media_command; FFmpeg testsrc2; rawvideo/NUT — 25/1 fps、16x16、6 frames。各 PTS を有理数で照合。 |
| `cfr-30-1` | generated | MIT OR Apache-2.0 | scripts/fixtures.py media_command; FFmpeg testsrc2; rawvideo/NUT — 30/1 fps、16x16、6 frames。各 PTS を有理数で照合。 |
| `cfr-30000-1001` | generated | MIT OR Apache-2.0 | scripts/fixtures.py media_command; FFmpeg testsrc2; rawvideo/NUT — 30000/1001 fps、16x16、6 frames。各 PTS を有理数で照合。 |
| `cfr-60000-1001` | generated | MIT OR Apache-2.0 | scripts/fixtures.py media_command; FFmpeg testsrc2; rawvideo/NUT — 60000/1001 fps、16x16、6 frames。各 PTS を有理数で照合。 |
| `vfr` | generated | MIT OR Apache-2.0 | scripts/fixtures.py media_command; FFmpeg testsrc2/select; rawvideo/NUT — PTS=0,1/30,1/10,1/5,1/3,1/2 秒。平均 fps で VFR を推定しない。 |
| `hdr-pq` | generated | MIT OR Apache-2.0 | scripts/fixtures.py media_command; original geq grayscale ramp; FFV1/Matroska — BT.2020 10bit limited range PQ metadata/decode input。輝度校正・tone mapping の正しさは保証しない。 |
| `hdr-hlg` | generated | MIT OR Apache-2.0 | scripts/fixtures.py media_command; original geq grayscale ramp; FFV1/Matroska — BT.2020 10bit limited range HLG metadata/decode input。輝度校正・tone mapping の正しさは保証しない。 |
| `noto-sans-cjk-jp` | external | OFL-1.1 | notofonts/noto-cjk Sans2.004; Noto Sans CJK JP Regular; Google/Adobe upstream — 日本語組版の固定フォント。emoji/IVS 全 coverage の保証はしない。 |
| `bframes` | generated | MIT OR Apache-2.0 | scripts/fixtures.py media_command; FFmpeg testsrc2 / native MPEG-4 (`-bf 2`) / NUT — 24 fps、6 frames、PTS=1/24〜6/24、B-frame reorder / drain / seek の整数 PTS 検証 |
