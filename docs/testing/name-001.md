# NAME-001 調査記録（途中）

状態: `in_progress`。2026-10-06、`codex/m5-completion`の調査途中の記録。名称の採用はADR-0042、残る照会・確保判断は[OQ-02](../open-questions.md#oq-02-商標の確認と名前の確保)で管理する。

## 実施済みの検索

[J-PlatPat 商標検索](https://www.j-platpat.inpit.go.jp/t0100)で出願・登録情報を対象に、商品・役務の区分を指定せず検索した。下記2条件は個別検索であり、AND結合していない。

| 検索項目 | 入力 | 表示結果 |
|---|---|---|
| 商標（検索用） | `Kronello` | 0件 |
| 称呼（類似検索） | `クロネロ` | 2件。登録6352701（区分35・36・41）と登録6824978（区分30） |

登録6352701は株式会社サン・クロレラ、登録6824978はかどや製油株式会社の結果で、検索時にはともに「存続-登録-継続」と表示された。これは指定した検索条件の結果であり、名称の利用可否・類似性の最終判断ではない。

ローカルの取得記録は`target/m5-acceptance/name-001/jplatpat-kronello.txt`と`jplatpat-phonetic.txt`。`target/`はGit管理外のため、再確認時は上記条件・登録番号で検索する。

## 米国・EUの検索

2026-10-06、[USPTO Trademark Search](https://tmsearch.uspto.gov/)のWordmarkに`Kronello`、Live/Deadとも対象、区分指定なしで検索し、読み込み後の`No results found`を確認した。[EUIPO eSearch](https://euipo.europa.eu/eSearch/#basic/1+1+1+1/100+100+100+100/Kronello)のbasic検索ではTrade marks 0件を確認した。検索条件は文字列のみで、類似綴り・全加盟国の国内登録の網羅調査ではない。ローカル取得記録は同じ証拠ディレクトリの`uspto-kronello.txt`と`euipo-kronello.txt`。

## crate・domainの登録確認

2026-10-06、公式crates.io APIで`kronello`とworkspaceの20 crate名を個別照会し、全21件がHTTP 404だった。現時点の公開登録は見つからず、当プロジェクトが確保済みという意味ではない。対象はanimation/audio/cli/eval/ffi/framebridge/gpu/jobs/mcp/media/model/platform/render/service/store/template/testkit/text/time/vectorの各`kronello-`名。[Verisign公式RDAP](https://rdap.verisign.com/com/v1/domain/kronello.com)もHTTP 404で、`kronello.com`の登録レコードは見つからなかった。登録可能性を予約・保証する応答ではない。取得時には再確認する。

照会URL・HTTP status・UTC日時は`target/m5-acceptance/name-001/registry-check.json`に保存した。workspaceは現在`publish = false`であり、名称のみを確保する公開は実施していない。

## 未完了

- 所有者の採用・確保判断: 未完了。登録・取得・購入は実施していない。

## 所有者判断の記録

2026-10-06、所有者へ確保アクションの判断を照会し、回答は「判断保留」だった。crates.io の名称登録・ドメイン取得などの確保・購入は実施しないで保留し、採用可否の最終判断も行われていない。照会結果の有効性は取得時点の再確認が前提である。

全対象の結果・検索範囲・確認日を揃え、必要な判断を記録するまでNAME-001をdoneにせず、OQ-02も未決のまま維持する。
