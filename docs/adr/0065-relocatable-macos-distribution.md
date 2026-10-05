# ADR-0065: macOS 配布 runtime を再配置・署名後に全体検証する

- 状態: 採用
- 日付: 2026-10-05
- 対象: RELEASE-001

## 背景

ADR-0018 / 0019 / 0035 / 0036 / 0048 / 0049 / 0050 の LGPL 動的ロード、単一 FFmpeg major、codec、worker 契約を維持する。開発 prefix の `libavcodec` 一つを `otool` で確認しても、配布 binary の起動、他の library、再配置、差し替え、署名の証明にはならない。既存 ADR の決定は置換しない。

## 決定

- `scripts/package_macos.py` が CLI `kronello`、MCP `kronello-mcp`、software codec 検証用 `release_roundtrip`、5 本の FFmpeg library と SVT-AV1 / dav1d、license 原文 / PATENTS、native manifest / receipt、build recipe、署名 entitlement、build provenance と配布後 hash inventory を組み立てる。FFmpeg executable、headers、pkg-config、static archive、開発用 library、不要な libavdevice / libavfilter は同梱しない。
- native input は `scripts/native-dependencies.json` と一致する自前 LGPL build のみ。元 library hash、全5本の ABI / license / configuration、SVT-AV1 / dav1d の実版を receipt と照合する。license / PATENTS は SHA-256 検証済みの upstream source archive の原文とも一致させる。runtime の install name / rpath の変更と署名で byte hash が変わるため、元 receipt と配布後 inventory を分ける。
- library の ID は `@rpath/<major-versioned-name>`、内部依存は `@loader_path/<name>`。executable の依存と rpath は `@executable_path/../lib`。全 Mach-O を走査し、この閉じた集合または Apple system library / framework 以外の依存と rpath を拒否する。各 executable は隣接する package manifest がある場合、その root の `lib/` を既定ロード先にする。欠落・ABI 不一致なら失敗する。明示した `KRONELLO_FFMPEG_LIB_DIR` が常に優先し、失敗時に system / build prefix へ戻らない。
- install name / rpath の変更後、library → executable の順に署名する。既定は ad-hoc (`--sign -`)。`--sign-identity` を明示した場合だけ host keychain の Developer ID identity を用い、timestamp と executable の Hardened Runtime を有効にする。差し替え契約を維持するため executable に `com.apple.security.cs.disable-library-validation` だけを付ける。GUI app の署名・entitlement は GUI 担当者が別途検証する。
- `scripts/verify_package.py` は元 package と native build prefix の外に新規 copy を作り、hash、全 Mach-O の `otool -L/-l/-D`、各署名の `codesign --verify --deep --strict`、実 CLI と MCP の `capabilities.get`、AV1 と ProRes/PCM24 の encode / decode / mux / PTS / sample 精度を検証する。別 directory の同一 ABI runtime copy への override と再 roundtrip、存在しない override の型付き失敗も確認する。任意の第三者 build すべての差し替え互換性を保証する検査ではない。
- 検証 report は platform / architecture / UTC、revision と dirty source hashes、package inventory SHA-256 と manifest SHA-256、全実行 command / 実 cwd / exit code / stdout / stderr を持つ。timeout は exit code を捏造せず null とし、途中の stdout / stderr も保存する。codec 検査は PTS / end / dimension に加え、frame ごとに異なる RGB の BT.709 参照値との全 YUV sample 比較、および PCM24 の全 channel sample 比較を含む。配布物外に保存し、失敗時も `status=failed` を残す。`--unsigned` は組み立て候補、`--static-only` は `acceptance_verified=false` の依存検査に限る。
- notarization / Gatekeeper の検証は Apple credential が必要な手動工程にする。ad-hoc 成功を notarized distribution と呼ばない。Windows / Linux の配布は、別の package と platform 上の独立検証が完了するまで保証しない。

## 影響と検証

配置は `bin/`、`lib/`、`tools/`、`licenses/` と root manifests に固定し、将来 GUI app の Resources 内へ directory 全体を埋め込める。`scripts/build_macos_app.py` や app bundle は本 ADR の実装対象に含めない。FFI host executable の既定 runtime 検索はこの CLI package 配置とは異なるため、GUI 統合時には同じ runtime directory を明示する。

手順と実施済み / pending host run の境界は [RELEASE-001 検証](../testing/release-001.md)。Apple の [Library Validation entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.cs.disable-library-validation) と [notarization 手順](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) を参照する。Developer ID / notarization の成功は今回の sandbox では未検証。
