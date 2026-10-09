# ADR-0136: カメラ RAW デコードとベンダー SDK 境界

- 状態: 採用
- 日付: 2026-10-08

## 背景

MEDIA-005 は BRAW/R3D/ProRes RAW を要求するが、BRAW（Blackmagic）
と R3D（RED）はプロプライエタリ SDK が必須で、ライセンス・配布
条件上リポジトリへ同梱できない。LibRaw（LGPL-2.1/CDDL デュアル）
は多数のカメラ RAW と DNG/CinemaDNG をデコードでき、配布ポリシー
に適合する。ProRes RAW は macOS の VideoToolbox 経由で追加経路を
持てる。実装範囲と境界を決める。

## 決定

- `native-dependencies.json` に libraw 0.22.2 を追加済み
  （LGPL-2.1 側で採用。openmp/lcms/jpeg/zlib は明示無効で
  vendored ビルドを決定的に保つ）。
- RAW スチルは `AssetKind::Image` として LibRaw 経由でデコード
  する。CinemaDNG シーケンス（連番 DNG）は既存の image sequence
  source 経路に RAW 判定を載せ、video 相当の stream として扱う。
  圧縮 DNG（lossy JPEG/deflate）は vendored 構成では未対応とし、
  遭遇時は `UNSUPPORTED_FEATURE` の型付き拒否（system ビルドでは
  libjpeg/zlib 連結済みの LibRaw があればデコード可）。
- ProRes RAW は framebridge の VideoToolbox/AVFoundation 経路に
  runtime feature-check で追加する。macOS のみ・OS/ハードで
  非対応の場合は型付き拒否。
- BRAW/R3D は vendor SDK adapter の境界のみ定義し、SDK 未検出時は
  `UNSUPPORTED_FEATURE` の型付き拒否。FFmpeg/LibRaw では
  デコードできないため黙って誤認しない（拡張子・magic で検出して
  明示エラーにする）。
- カラーは DNG metadata の primaries/AsShotNeutral から
  scene-referred へ決定的に変換し、working color space
  （ADR-0024）へ入れる。LibRaw の内蔵 auto WB/gamma は使わず、
  係数を明示して決定的にする。デモザイク方式は versioned パラ
  メータとして固定する。
