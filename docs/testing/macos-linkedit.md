# macOS release dylib の LINKEDIT 検証

2026-10-06、Rust 1.95.0 / Xcode 27 の組合せで `build_macos_app.py --release` の Swift リンクが `mis-aligned LINKEDIT string pool` に失敗した。コピー元の Cargo dylib とコピー先の SHA-256 は一致し、`install_name_tool` による dylib の変更はなかった。

Rust の release 既定 `-C strip=debuginfo` は LLVM の Mach-O 情報除去を通り、間接シンボル表の後の文字列プールが 8 byte 境界に揃わない場合がある。FFI の除去済み artifact の `LC_SYMTAB.stroff` は `17089380`（8 の剰余 4）だった。外部シンボル `environ` を参照する最小 dylib を同じ Rust、`opt-level=3` で作ると、除去ありだけが `ctypes.CDLL` に同じ理由で拒否され、除去なしでは読み込めた。[Rust issue 157750](https://github.com/rust-lang/rust/issues/157750)、[LLVM issue 203678](https://github.com/llvm/llvm-project/issues/203678) と一致する。

`scripts/build_ffi.py` は macOS release の `kronello-ffi` package に限り Cargo override `profile.release.package.kronello-ffi.strip="none"` を指定する。Rust の版、release 最適化、依存 crate、CLI の profile、通常の `install_name` と署名・検証は維持する。バイナリの後処理で不正な配置を書き換えない。代償として dylib のデバッグ情報・シンボルによるファイルサイズが増える。上流修正を含む Rust に移行した際は同じ受け入れ手順で override の撤去を判断する。

再現・受け入れ手順:

最小再現は次のソースを `minimal.rs` として保存する（macOS の外部シンボルを 1 個参照し、間接シンボル表の配置を再現する）。

```rust
unsafe extern "C" { static mut environ: *mut *mut u8; }
#[unsafe(no_mangle)]
pub extern "C" fn answer() -> usize { unsafe { environ as usize } }
```

```sh
rustc --edition=2024 --crate-type=cdylib -C opt-level=3 -C strip=debuginfo minimal.rs -o minimal-stripped.dylib
rustc --edition=2024 --crate-type=cdylib -C opt-level=3 -C strip=none minimal.rs -o minimal-unstripped.dylib
python3 -c 'import ctypes; ctypes.CDLL("./minimal-stripped.dylib")'
python3 -c 'import ctypes; ctypes.CDLL("./minimal-unstripped.dylib")'
```

この環境では前者だけが `fileOffset=0x8054` で失敗した。

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer python3 scripts/build_macos_app.py --release
otool -l apps/macos/Libraries/libkronello_ffi.dylib
codesign --verify --deep --strict target/macos/Kronello.app
```

修正後の実際の Cargo package override artifact は `stroff=17141856`（8 の剰余 0）。実際の Xcode の Swift executable リンク、app の通常の厳格な署名検証、コピー先 dylib の `ctypes.CDLL` 読み込みが成功し、ビルド全体の終了コードは 0。手元の実行ログは `target/m4-acceptance/final-macos-app-build-linkedit-fix.log`。
