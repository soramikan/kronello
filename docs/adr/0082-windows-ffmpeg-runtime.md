# ADR-0082 Windows の FFmpeg runtime と実 worker の検証

- 状態: 採用
- 日付: 2026-10-06
- 対象: MEDIA-003
- 部分置換: ADR-0074 の Windows 検証範囲。worker の detach / publication 契約は維持する。

## 決定

Windows の C shim は `LoadLibraryExW` / `GetProcAddress` / `FreeLibrary` で
明示 directory の `avutil-61.dll` / `avcodec-63.dll` / `avformat-63.dll` /
`swscale-10.dll` / `swresample-7.dll` を開く。Rust の UTF-8 path を UTF-16 に
変換し、依存 DLL の検索は指定 DLL の directory と Windows の既定の安全な検索範囲に限る。
失敗時に別 runtime へ戻らない。library / child の所有関係と ABI / LGPL 検証は維持する。

Windows MSVC target の shim は Clang でコンパイルする。build-time の
`KRONELLO_FFMPEG_PREFIX` が headers と `bin/` の DLL directory を指定する。
FFmpeg 自体は MinGW で共有 library としてビルドする。FFmpeg の import library に
Kronello をリンクしない。既存の source SHA-256 / version / configure / LGPL policy を
Windows でも使い、SVT-AV1 と dav1d を含む自前 runtime を CI の入力とする。

CI は full CLI / MCP をビルドし、capabilities の distribution 検査、AV1 / ProRes の
PTS・画素比較、PCM24 の sample 比較、および実際の `kronello worker --job` が CLI 終了・
MCP EOF 後に完了するテストを実行する。job 層の固定 payload テストで代替しない。
OS / revision / command / exit / log / native receipt を artifact に残す。

## 保証範囲

CI 手順の追加だけで Windows を保証経路に昇格しない。Windows 上の成功 artifact が
受け入れ証拠であり、それまでは MEDIA-003 は `in_progress` とする。
Windows の配布 package・署名・GUI はこの決定の対象外。
