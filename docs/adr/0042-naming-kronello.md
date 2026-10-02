# ADR-0042: 名称を Kronello に変更する

- 状態: 採用
- 日付: 2026-10-02
- 置換対象: [ADR-0023](0023-naming-cinewright.md)

## 背景

ADR-0023 で名称を Cinewright としたが、採用後に候補名単独で検索し直したところ、同名の事業者が見つかった。米国メイン州でドキュメンタリー映画制作のワークショップを運営する Cinewright（cinewright.com）である。ソフトウェアではないが映像制作という隣接分野であり、`.com` ドメインも取得できないため、名称を再検討した。

採用時の調査は、複数の候補名を OR でまとめて検索し、ドメインは DNS の応答だけで判断していた。今回は候補ごとに単独で検索し、`.com` は whois で確認した。

## 決定

- プロダクト名は Kronello（chrono に由来する造語）。
- CLI の実行ファイルは `kronello`、crate の接頭辞は `kronello-`、プロジェクトファイルの拡張子は `.kronello`。
- `cinewright`、`koma`、`ved` という表記は新しく書かない（`docs/archive/`、ADR-0013、ADR-0023、`naming.md` を除く）。
- GitHub リポジトリは `soramikan/kronello` とする。

## 影響

- 調査時点で、crates.io・npm・PyPI・Homebrew に `kronello` はなく、GitHub に同名のリポジトリはなく、`kronello.com` は whois で未登録、単独の Web 検索でも同名の製品・企業は見つからなかった。
- 綴りの近い名称として Kronel（ブラジルの衛生用品）と Kronell（ハンガリーの建設会社）があるが、別分野である。
- 商標データベース（J-PlatPat、USPTO、EUIPO）は未照会。public にする前に確認する（OQ-02）。
- crates.io の名前とドメインは先着順のため、早めに確保する（OQ-02）。

## 関連

- [名称の衝突調査](../naming.md)
- [11 ワークスペース](../architecture/11-workspace.md)
- [未決事項](../open-questions.md)
