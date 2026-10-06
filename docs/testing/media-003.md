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

## Windows 実受け入れ証拠

[run 37417802067](https://github.com/soramikan/kronello/actions/runs/37417802067) の
Windows job `112120204171`、head `b98af22`、実 checkout merge SHA
`8bee690a2dd2790f70733de4966294978137bff4` で受け入れ対象を実行した。
Windows Server 2025 / AMD64、Rust MSVC binary が自前 MinGW shared runtime を読み込み、
6 commands（Rust version、distribution capabilities、roundtrip、CLI / MCP の 3 tests）すべて exit 0。
3 tests は各 `1 passed; 0 failed; 0 ignored` であり、空 filter・skip はない。

AV1 は `libsvtav1` encode / `libdav1d` decode、ProRes は `prores_ks` encode / `prores` decode。
BT.709 / limited range と有理数 PTS を検査し、104,448 video samples の最大誤差は
8-bit 単位で `0.42517692878523405`。PCM24 は stereo / 48 kHz の 16,000 samples を検査し、
ProRes + PCM24 mux の streams・時刻・snapshot hash tags も一致した。
CLI 親終了と MCP EOF の後に、固定入力の実独立 worker の成果物と再接続 query を確認した。

receipt は FFmpeg `9.0.2` / LGPL-2.1-or-later、SVT-AV1 `4.2.0`、dav1d `1.5.4` の
pinned archive hashes、5 FFmpeg library の LGPL / ABI、license files と各 DLL SHA-256 を記録する。
必要な MinGW runtime DLL も同じ `bin/` に配置し、GPL / nonfree は無効。
[検証記録 JSON](media-003-windows-measurements.json) に command records、roundtrip、receipt、
artifact log hashes を保存した。全文 log は run の `media-003-Windows-X64` artifact にある。

初回 bare `bash` が System32 の WSL launcher に解決された失敗は、absolute MSYS2 bash と
`uname -s` 検証で修正した（7 Python regressions 成功）。その後の Windows 実行で
canonical `\\?\` path に DLL 名を `/` で連結する失敗を確認し、`b98af22` で
Windows separator を正規化した。extended prefix は保持し、
`LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS` は変更していない。
上記 capabilities の実成功は、この旧失敗 path の回帰検証でもある。

MEDIA-003 の Windows 固有受け入れ証拠は取得済み。最終 head `fde889e` の
[run `37418631507`](https://github.com/soramikan/kronello/actions/runs/37418631507)、
job `112122784486`、checkout merge `275380d2eb41b15f3b7c94a0d2115c252a9a5c26` でも
同じ 6 commands が exit 0、3 tests が各 `1 passed; 0 ignored` で成功した。
最終 verification records も耐久 JSON に追記した。Windows job 全体も `success`。
storage / time / jobs tail は 63 passed / 0 failed / 2 ignored で、実 Windows parent Job での
breakaway 拒否後の worker 生存、publication contention、heartbeat、kill / cancel / FIFO を通過した。
2 ignored は optional storage fixture と別 volume 指定が必要な test で、上記 MEDIA 必須検証に skip はない。
M4 全体の最終 CI 完了とは区別する。
