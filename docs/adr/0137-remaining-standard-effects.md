# ADR-0137: 残エフェクト群の棚卸しと追加方針

- 状態: 採用
- 日付: 2026-10-08

## 背景

FX-008 は未実装の標準エフェクトの棚卸しと versioned effect としての
追加を要求する。既存は blur/drop_shadow/glow/sharpen/vignette/
corner_pin/stabilize、keying(chroma/luma)、color(exposure/levels/
curves/hsl/lut)、blend・transition 群、audio 7 種 + plugin。
どの集合を M10 で追加し、どう保証するかを決める。

## 決定

- 着手時に標準エフェクト一覧（NLE/MG の代表的集合）と実装済みの
  対応表を受け入れ記録へ残し、採用・見送りの理由を明記する。
- M10 で追加する映像エフェクト（各 versioned descriptor、
  CPU reference + GPU shader + golden）:
  - `kronello.grain`: seeded 決定的フィルムグレイン（amount/size/
    monochrome、乱数は固定 seed・時間連続性を保つ）
  - `kronello.mosaic`: ピクセル化（block size、center/edge 基準）
  - `kronello.invert`: RGB/alpha 保持の反転（channel 選択可）
  - `kronello.channel_mixer`: 4x4 係数行列のチャンネル混合
  - `kronello.tint`: map black/white to 色のトーン付け
  - `kronello.directional_blur`: 角度+長さのモーションブラ
  - `kronello.radial_blur`: spin/zoom 型放射ブラ（center 指定）
  - `kronello.displace`: 別レイヤ輝度/チャンネルによる変位マップ
  - `kronello.generate`: gradient（linear/radial）・checkerboard・
    grid の生成（ソース不要の synthetic layer）
- 追加する音声エフェクト:
  - `kronello.audio.delay`: ディレイ/エコー（delay time・feedback・
    wet/dry、整数サンプル単位）
  - `kronello.audio.reverb`: 生成 IR の決定的畳み込み（room 係数で
    IR をアルゴリズム生成。外部 IR 取り込みは後続）
  - `kronello.audio.pitch`: ピッチシフト（半音単位。WSOLA 基盤を
    再利用）
  - `kronello.audio.gate`: ノイズゲート（threshold・attack・release・
    hysteresis）
- 全エフェクトで式・アニメーション対象パラメータは既存 descriptor
  規約に従い、GPU 非対応の組合せは型付き拒否か CPU reference に
  明示的に落とす。黙って近似しない。
