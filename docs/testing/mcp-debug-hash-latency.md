# macOS CIのMCP応答待ちとdebug SHA-256

PR #21の初回macOS job `114969359539`（run `38296693699`）は、`public_api_fixture_commands_return_schema_valid_success_from_real_binary`のMCP応答待ち20秒で失敗した。17 passed/1 failed、成功扱いにしない。旧harnessは要求名を表示せず、remoteで待っていた個別要求は断定できない。原ログは`target/perf-014-ci-macos-job.log`へ保持した。

同じfixtureの要求別計時では、通常要求が約0.2〜35msなのに対し、日本語`template.preview`が3.760〜3.856秒、before/afterを評価する`template.migration_plan`が7.562〜7.572秒だった。二つのlegacy protocolのfixtureを通過し、各stderrは1175bytesだった（`target/perf-014-mcp-timeout-diagnostic.log`）。stderr pipe詰まりを原因とは扱わない。

`kronello-text`はfont lockを検証するとき全font bytesのSHA-256とface identityを照合する。sha2 0.10.9はasm featureを使わないaarch64でportable圧縮関数を選び、debugではこの大きなfont検証が重い。root Cargo設定で`[profile.dev.package.sha2] opt-level=3`を指定した。同じ入力と同じhash/identity検証を保ち、release設定、MCP仕様、応答待ち20秒は変更しない。

同じfixtureは最適化後にpreviewが192.773〜197.713ms、migrationが390.528〜393.178ms、全testが4.32秒で通過した（`target/perf-014-mcp-hash-optimized.log`）。ホスト全体の静穏やremoteの単一根因は断定しない。harnessに要求名付きtimeoutと任意の`KRONELLO_TEST_MCP_TRACE=1`診断を追加し、再発時には個別要求を判別できる。最終workspace/CIの結果はPRと最終検証記録へ追記する。
