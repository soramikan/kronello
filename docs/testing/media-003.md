# MEDIA-003 Windows FFmpeg runtime

## 実装

[ADR-0082](../adr/0082-windows-ffmpeg-runtime.md) の Windows DLL loader、Clang shim build、
自前 LGPL runtime の MinGW build と CI evidence runner を追加した。
`KRONELLO_FFMPEG_PREFIX` は headers / DLL の build-time prefix、
`KRONELLO_FFMPEG_LIB_DIR` は runtime の明示 directory。Windows は `bin/` を使う。

## 再現手順

Windows CI の `Windows (full CLI/MCP and LGPL media)` job は MSYS2 の MINGW64 Python /
GCC / CMake / Meson で次を実行する。source archive の取得は既存 manifest の SHA-256 で照合する。

```text
python scripts/build_ffmpeg_lgpl.py --jobs 3
cargo build -p kronello-cli -p kronello-mcp --all-targets --locked
python scripts/media_003_evidence.py
```

後半の Cargo は pinned Rust MSVC toolchain と Clang を使用する。
`media_003_evidence.py` は以下を実行し、失敗・空 test filter では非ゼロ終了する。

- `capabilities --verify-distribution`: 全 5 library の ABI / LGPL / codec 能力。
- `release_roundtrip`: AV1 / ProRes decode の色タグ・画素・有理数 PTS、ProRes + PCM24 mux と 16,000 audio samples。
- CLI の `cli_exit_detaches_worker_and_preserves_project_bytes_and_mtime`: 親 CLI 終了後に gate を解放し、実 worker の成果物を確認。
- MCP の `submitted_job_survives_mcp_eof_and_is_queryable_on_new_connection`: MCP 終了後の実 worker 継続と再接続 query。
- MCP の `versions_negotiate_and_registry_schemas_are_self_contained`: 実 binary の capabilities を含む schema 検査。

`target/media-003-evidence/evidence.json` と各 command log、
`target/native/ffmpeg-lgpl/build-receipt.json` を CI artifact に保存する。

## 受け入れ状態

Windows 実行 artifact は未取得。CI の追加を成功証拠として扱わず、MEDIA-003 は
`in_progress` を維持する。macOS の互換性チェック結果は Windows の保証に代用しない。


初回 Windows CI は FFmpeg configure の bare `bash` が System32 の WSL launcher に解決され失敗した。
CI で `KRONELLO_MSYS2_BASH=C:/msys64/usr/bin/bash.exe` を指定し、builder は absolute path と
`uname -s` の MSYS/MINGW 判定後、その実行ファイルを使う。7 Python regressions が成功した。
修正コミット `23dde7a` の CI は GitHub account billing の制限で step 開始前に停止しており、
修正済み Windows runtime の受け入れ証拠はまだ得られていない。
