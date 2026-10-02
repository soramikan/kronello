# ADR-0036: リリースには自前の LGPL ビルドの FFmpeg を同梱する

- 状態: 採用
- 日付: 2026-10-02

## 背景

FFmpeg は LGPL 構成を動的リンクする（ADR-0018）。Homebrew などのシステムの FFmpeg は GPL 構成でビルドされていることが多く、版と構成が利用者ごとに異なる。

## 決定

- リリースの配布物には、版と構成を固定した LGPL ビルドの共有ライブラリを同梱する。
- ビルドスクリプトと native dependencies manifest（版、configure オプション、hash）をリポジトリで管理する。
- 開発時は、pkg-config で見つけたシステムの FFmpeg でもビルドできる。
- 対応する FFmpeg は単一のメジャー版に固定する。
- 利用者は環境変数で別の FFmpeg に差し替えられる。差し替えた構成は `capabilities.get` で報告する。

## 影響

- 配布物の結果が再現でき、GUI の利用者に FFmpeg のインストールを求めない。
- FFmpeg のビルドと更新を自分たちで保守する必要がある。
- 開発環境とリリースで FFmpeg の構成が異なりうるため、リリース前の検証は同梱ビルドで行う。
- 検討した代替案: 常にシステムの FFmpeg、常に同梱版のみ。

## 関連

- [12 プラットフォームと依存](../architecture/12-platform-dependencies.md)
- [ADR-0018](0018-ffmpeg-lgpl-dynamic-linking.md)
