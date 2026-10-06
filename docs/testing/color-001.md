# COLOR-001 検証記録

## 契約と対象

ADR-0086 に従う。203-nit 基準白、PQ/ST2084、HLG 1000-nit/gamma 1.2 を固定する。CPU/GPU working RGB は linear Rec.2020、alpha は premultiplied、display の SDR tone map を numeric/HDR movie に焼き込まない。

native HDR 入力は明示 Rec.2020 PQ/HLG の planar 10-bit YUV 3 format。HDR 出力は ProRes HQ 10-bit MOV + PCM24 の閉じた version 1 profile。その他の native format / color contract / codec は型付き未対応であり、8-bit、SDR、別 codec へ継続しない。strict resident HDR は型付き未対応。CPU decode + 明示 GPU upload は対応する。

## 再現手順と証拠

- `cargo test -p kronello-render --lib hdr::tests --locked`: 基準白 PQ code 約 0.580689、HLG 75% 約 203 nits、RGB OOTF の往復、negative/peak 超過拒否。
- `cargo test -p kronello-media --test hdr native_ten_bit_pq_hlg_precision --locked`: PQ/HLG の RGBA64→ProRes10-bit→native decode、locked format/tags/hash、composition render、固定 snapshot JSON/hash、最終 HDR movie/probe/decode。display tone map が焼き込まれない。native 1024 code ramp は 256 階調を超えて保持。明示 SDR profile だけが約 0.5 linear SDR へ変換される。非対応 H264 HDR と異なる transfer profile は出力前に拒否。SDR source white→HDR 203 nits、半透明 PNG の premultiplied alpha と final opaque 合成、負の authored HDR RGB / >1 保持、非表現域 HLG 飽和青の型付き拒否も含む。2026-10-06 最終版: CPU/GPU 計 2 tests pass、2.24 秒。
- `cargo test -p kronello-media --test hdr native_ten_bit_pq_hlg_actual_gpu --locked`: 実 Metal GPU と CPU の全画素を比較、誤差 1/1024 未満。sandbox 外の実 GPU で最終 CPU/GPU 2 tests pass、2.24 秒。負の RGB と半透明 HDR source も GPU 比較する。
- `cargo test -p kronello-cli --test jobs color001_hdr --features kronello-service/test-job-control --locked`: 同期 export と実 detached worker を PQ/HLG 各々実行。固定 profile、HDR 意味版 1、required feature を確認し、project を削除して worker を解放。3 frame を完了し、native 10-bit tags/probe/hash と同期出力の全 source plane bytes が一致。2026-10-06: 1 test pass、1.38 秒。
- `cargo test -p kronello-render --test render color001_legacy --locked`: HDR 版欠落 legacy snapshot の JSON/hash を保持し、HDR profile の意味版欠落・未来版を拒否。metadata は reference white 203 / HLG peak 1000 を返す。2026-10-06: 1 test pass、2.62 秒。
- `cargo test -p kronello-render --test render color001_8k --locked -- --ignored --nocapture`: 7680×4320、33,177,600 pixels を tile で実行し row-major RGBA16F artifact を 265,420,800 bytes 保存。日本語 text、半透明 alpha matte、HDR bright zero-offset shadow blur による glow を含む。HDR >1 と coverage alpha を確認。実 GPU で最終 2 ROI 版が 1 test pass、67.07 秒。通常の headless test suite ではこの host acceptance test を ignore し、上記の明示コマンドで実行する。

## 8K の画素・資源・成果物

8K の初期 debug CPU 全面実行は数分以上継続したため停止し、実 GPU 全面実行と同じ pixel scale の CPU ROI oracle を採用した。最終シーンは 1024×576 design extent を 7680×4320 へ **7.5 px/design unit** で拡大する。12-unit の日本語 font は約 90px。text の position (20,5) は (150,37.5) pixels に対応する。作品内容は意図的に左上に配置するが、出力と保存は全 33,177,600 pixels。

CPU oracle は (150,40) と (375,40) の各 64×64 pixels。前者は glyph の AA/明るい blur glow、後者は x=405（design x=54）の半透明 matte boundary が最終文字を横切る部分を検証する。CPU/GPU の 4 channels を誤差 0.01 未満で比較し、matte の外側を alpha 0 と確認する。HDR >1、半透明 coverage、境界外の透明領域を保存後も確認する。root は元解像度の 512×512 tile preview を視覚確認済みで、文字/glow/意図的な matte clip に破損・継ぎ目を認めない。

成果物は `target/m4-acceptance/color-001/8k-linear.rgba16f`（row-major RGBA16F 265,420,800 bytes）、`8k-display.png`（7680×4320）、`8k-top-left-tile-display.png`（元画像の正確な 512×512 tile）。numeric は tone map しない。display は SDR 表示変換後の画素から生成する。

`/usr/bin/time -l` で compiled test binary を直接実行した計測（matte boundary の第二 ROI 追加前、同じ全 8K 実行）は **62.09 秒、maximum RSS 149,438,464 bytes、peak memory footprint 361,022,112 bytes、swap 0**。cargo/build を含む計測の RSS と混同しない。linear/display の最大 tile payload は 8,388,608 bytes。最終 2 ROI 版の描画・保存・PNG 検証は 67.07 秒。追加 ROI は 64×64 のみで全フレーム surface を追加しない。

## 最終 gate

`cargo clippy -p kronello-render -p kronello-media -p kronello-service --all-targets --locked -- -D warnings` は成功。schema/Swift は root が共通 API から再生成した。HLG inverse OOTF 後に code が [0,1] を超える場合は quantizer へ進まず `UNSUPPORTED_FEATURE` とし、実 movie export の出力未公開も確認した。f64 endpoint の 1e-12 以下の丸めだけを補正し、gamut の clamp としない。
