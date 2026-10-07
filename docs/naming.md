# 名称の衝突調査

調査日: 2026-10-02。結論は [ADR-0042](adr/0042-naming-kronello.md)（名称を Kronello に変更）。`koma`、Cinewright の順に採用と撤回を経ている。

調べた範囲は crates.io、npm、PyPI、Homebrew（formula / cask）、GitHub のリポジトリ検索、Web 検索。
2026-10-02時点では商標データベース（J-PlatPat、USPTO、EUIPO）は未照会だった。以下は当時の記録である。

2026-10-06追記: 以下の名称調査は当時の記録であり、現在の商標・domain/crateの利用可否を保証しない。J-PlatPat・USPTO・EUIPOの限定検索と公式APIによるcrate/domain登録照会を[調査記録](testing/name-001.md)へ追記した。所有者の採用・確保判断と取得は未完了で、[OQ-02](open-questions.md#oq-02-商標の確認と名前の確保) / NAME-001で追跡する。

## `koma`（撤回）

| 対象 | 結果 |
|---|---|
| crates.io | `koma`、`koma-cli`、`koma-model`、`koma-time`、`koma-core` はいずれも未登録 |
| Homebrew | なし |
| npm | 取得済み（別物の Web フレームワーク系モジュール、2022 年以降更新なし） |
| PyPI | 取得済み（別物、macOS 関連の AI 向けパッケージ） |

同名・類似名のソフトウェア:

| 名前 | 内容 | 衝突の種類 |
|---|---|---|
| [aula-id/koma](https://github.com/aula-id/koma)（[koma.run](https://koma.run/)） | Rust 製の AI コーディングエージェント。2026-06 開始、★60、活発 | 実行ファイル名 `koma` が同じ |
| [yukiteruamano/koma](https://github.com/yukiteruamano/koma) | Go 製の CLI マンガダウンローダー | 実行ファイル名 `koma` が同じ |
| [KOMA KOMA](https://komakoma.org/) | コマ撮りアニメーションアプリ。2010 年から。iPad / Windows / Mac / Web（[KOMA KOMA×日文](https://www21.nichibun-g.co.jp/komakoma/)）。教育現場で普及 | 同じ分野・日本市場での混同 |
| [Dytschgo/koma-motion](https://github.com/Dytschgo/koma-motion) | 「Koma Motion」。AI でスライドとモーションを作るデスクトップアプリ。2026-09-29 作成、★0 | 同じ分野 |
| KOMA-Script | 著名な LaTeX 文書クラス | 検索での埋没 |
| [kyonifer/koma](https://github.com/kyonifer/koma)、[KomaMRI.jl](https://github.com/JuliaHealth/KomaMRI.jl) | Kotlin の科学計算ライブラリ、MRI シミュレーター | 検索での埋没 |

実行ファイル名の衝突と、同じ分野の既存製品との混同の両方があるため撤回した。

## `cinewright`（撤回）

採用時の検索は他の候補名と OR でまとめていたため、同名の事業者を見落とした。単独で検索し直して見つかったため撤回した。以後の調査では、候補ごとに単独で Web 検索し、`.com` は whois で確認している。


| 対象 | 結果 |
|---|---|
| crates.io | 未登録 |
| npm | 未登録 |
| PyPI | 未登録 |
| Homebrew | formula / cask ともなし |
| GitHub | 同名のリポジトリは検索に現れず |
| Web 検索（採用時） | 他の候補とまとめた検索では同名の製品・サービスが見つからなかった |
| Web 検索（採用後、単独で再検索） | **同名の事業者あり。** 米国メイン州の [Cinewright](https://cinewright.com/who-we-are/)（cinewright.com）。ドキュメンタリー映画制作のワークショップを運営。ソフトウェアは提供していない。サイト上に ™ / ® の表記は見当たらないが、商標登録の有無は未確認 |

拡張子:

| 候補 | 結果 |
|---|---|
| `.cwr` | 不採用。SAP Crystal Reports のレポート、SolidWorks Simulation の結果ファイルなどが使用 |
| `.cinewright` | 採用 |

## `kronello`（採用）

chrono（時間）に由来する造語。

| 対象 | 結果 |
|---|---|
| crates.io / npm / PyPI / Homebrew | いずれも未登録 |
| GitHub | 同名のリポジトリは検索に現れず |
| `kronello.com` | whois で未登録 |
| Web 検索（単独） | 同名の製品・企業は見つからず |
| 綴りの近い名称 | Kronel（ブラジルの衛生用品ブランド）、Kronell Kft.（ハンガリーの建設会社）。いずれも別分野 |
| 商標データベース | 未照会 |

## 調査方法についての注意

- DNS に応答がないドメインでも、whois では登録済みのものが多かった。5〜6 文字の造語の `.com` はほぼすべて登録済みである。
- 複数の候補名を OR でまとめた検索は、個々の名前の衝突を見落とす。
- パッケージレジストリが空いていても、同名の事業者や製品が存在することがある。

## 検討した他の候補

いずれも各レジストリの登録状況と GitHub の上位リポジトリを確認した。

| 候補 | 状況 |
|---|---|
| Twenora、Sekvaro、Tweenza、Twenello、Tweniva、Sekvito、Reelello、Cutanta、Temexo | 全レジストリ空き、`.com` は whois で未登録、GitHub に同名なし。Twenora・Sekvaro・Tweenza は単独検索でも同名の製品なし |
| Tweenloom、Timelathe、Glyphreel、Kinetitle、Banctitre、Tweenwright、Holdcel | 全レジストリ空き、`.com` は whois で未登録、単独検索で同名の製品なし。Banctitre はフランス語の一般名詞 |
| Sekvenza | 空きだが、綴りの近い Sekvenca（クロアチアの映画制作会社）がある |
| Tweenery | 同名の米国 SNS 企業（閉鎖済み）があった |
| Cutloom | 同名の Web 動画エディターと macOS の CAM アプリがある |
| Tweencel | 同名の Aseprite 拡張がある |
| Kinewright、Reelwright、Timewright、Shotwright、Raccord | 同名のリポジトリ、または `.com` の使用がある |
| Roughcut、Finecut、Workprint、Dropframe、Onionskin など映像用語の実在語 | `.com` が使用中、または同名の製品がある |
| Hakobi、Komaori、Tokiori | 全レジストリ空き。和風の名前は見送り |
| Tweenery | 全レジストリ空き、同名の製品なし。モーション寄りでカット編集の印象が薄い |
| Multiplane | 全レジストリ空き。OpenToonz、TVPaint、Toon Boom などが機能名として使う一般語 |
| Pegbar | 全レジストリ空き。同名の手描きアニメ OSS と、同名のアニメスタジオが存在 |
| Intercut | レジストリは空きだが、同名の動画関連製品が複数存在（intercut.ai など） |
| Timewright、Cutwright、Smashcut、Mutoscope、Xsheet、Dopesheet、Trimbin | レジストリは空き。Web での詳細調査は未実施 |
| Flatbed、Sprocket、Splicer、Interlock、Conform、Armature、Motus、Tessera、Kinema、Subframe、Rostrum | crates.io ほかで取得済み |

## 出典

- [KOMA KOMA LAB](https://komakoma.org/)
- [KOMA KOMA×日文（日本文教出版）](https://www21.nichibun-g.co.jp/komakoma/)
- [KOMA KOMA for iPad（App Store）](https://apps.apple.com/us/app/koma-koma-for-ipad/id635794784)
- [Dytschgo/koma-motion](https://github.com/Dytschgo/koma-motion)
- [koma.run](https://koma.run/)
- [yukiteruamano/koma（pkg.go.dev）](https://pkg.go.dev/github.com/yukiteruamano/koma)
- [CWR File Extension（filext.com）](https://filext.com/file-extension/CWR)
- [intercut.ai](https://www.intercut.ai/)
- [danallison/PEGBAR](https://github.com/danallison/PEGBAR)
- [Cinewright（cinewright.com）](https://cinewright.com/who-we-are/)
- [Tweenery（Crunchbase）](https://www.crunchbase.com/organization/tweenery)
- [FrankOrozcoGT/cutloom-web](https://github.com/FrankOrozcoGT/cutloom-web)
- [Tweencel for Aseprite](https://devkidd.itch.io/tweencel)
- [Sekvenca](https://sekvenca.hr/)
- [Kronell Kft.](https://www.facebook.com/kronellkft/)
