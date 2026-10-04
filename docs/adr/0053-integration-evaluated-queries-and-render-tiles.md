# ADR-0053: 縦断デモの評価 query と大解像度の tile 実行

- 状態: 採用
- 日付: 2026-10-04
- 対象: INTEGRATION-001

## 背景

日本語 lower-third の CLI 作成、MCP 検査、固定 Sequence snapshot の 4K 出力を接続する。従来の scene.query は authored 構造、property.sample は組版入力を持たない evaluator の値を返していた。template の公開文字・色・背景帯寸法を renderer と同じ値で問い合わせる入口が必要である。また全画面の全 glyph / effect 面を保持する backend の 512 MiB 予算は、4K の小さい字幕でも超過する。

既存 ADR の時間・色・instance 分離・固定ジョブ入力・予算・backend 選択は維持する。保存文書と通常の組版 bounds の意味を変更しない。

## 決定

- `scene.query` に任意の `evaluation: {time, fonts}` を追加する。省略時は従来の構造 query を保持する。指定時は同じ保存 revision の RenderSnapshot、明示した font lock / locator、`build_scene_ir` を使い、active node に `evaluated` を返す。内容は最終 Property 値、公開入力反映後の text、text-local `layout_bounds`、world_transform、resolved effects。inactive node は構造結果に残し、`evaluated` は付けない。GPU を初期化しない。
- `property.sample` の任意 `fonts` を指定した場合は、同じ compiler の active node 値を times 順に返す。空配列も明示選択であり、必要 font を欠けば `FONT_MISSING`。この mode は node key に限定し、inactive node・欠落 Property・Composition key は `INVALID_REQUEST`。省略時の従来 evaluator mode（Curve / Composition input 等）は保持し、template の組版由来値を示す用途には使わない。
- 新しい font locator にも共有 local path 検証を適用する。hash / face / identity の失敗を代替 font で続行せず、renderer と同じ型付きエラーを返す。query で作品の revision を変えない。
- `capabilities.get.effects` は実装済み `kronello.gaussian_blur` / `kronello.drop_shadow` を列挙する。編集 registry にも同じ effect descriptor を登録し、shadow 付き authoring 内容を template.define / edit.plan が未知 descriptor として拒否する欠落を修正する。
- 出力の幅または高さが 512 pixels を超える frame は最大 512×512 pixels の tile に分ける。各 tile の origin / extent は元要求の画素格子から導出し、既存 DAG の逆方向 ROI / halo compiler と同じ selected backend を使う。linear / display を row-major の最終面へ組み立て、metadata は元の要求を記録する。失敗の retry / CPU fallback は設けない。個々の tile と halo の既存 pixel / surface 予算は維持する。
- tile 実行は大解像度の中間面を抑える変更であり、streaming export・最終面全体のメモリ削減・任意の巨大 halo の保証ではない。最終 linear / display 面は全画面のまま保持する。
- デモの納品は ADR-0050 の `image_sequence`。14 秒の Sequence を明示 `1/8 fps` の絶対格子で sampling し、0 秒の A と 8 秒の B の 2 frames を出す。通常の 24 fps 動画・ProRes・音声 mux の受け入れは本デモに含めない。backend 既定 GPU、4K 既定、CPU は明示選択のみ。

## 検証と影響

共有 query の font / inactive / local path / 不変性と、tile 境界をまたぐ shadow + blur の CPU 全画素比較を回帰テストにする。実 CLI / MCP の driver は instance 独立性、保護 retime、帯の両寸法、shadow の alpha、投入後編集、overflow の非公開を検査する。

通常の `layout_bounds` は固定 wrap_width と行数×line_height の矩形である。デモの帯幅は wrap_width + 横 padding を確認し、長い二行文字で帯高の増加を確認する。tight ink に追従する別仕様へ変更しない。

4K GPU の実機結果は supervisor が [INTEGRATION-001 検証](../testing/integration-001.md) に記録する。CPU や tile の単独回帰を GPU 受け入れの代替にしない。
