# ADR-0048: FFmpeg ABI 境界・同梱ビルド・素材検証を固定する

- 状態: 部分置換（[ADR-0062](0062-video-generator-and-timeline-edits.md): SDR 動画の色変換境界だけ。native plane / HDR 保持、LGPL、ABI、hash と seek の契約は維持）
- 日付: 2026-10-03
- 対象: MEDIA-001

## 背景

ADR-0018 / 0036 の動的リンク・差し替え、ADR-0028 の hash 照合について、具体的な構成と実装境界を決める。既存 ADR の方針は変更しない。

## 決定

- 配布用は FFmpeg **9.0.2** と SVT-AV1 **4.2.0**、dav1d **1.5.4**。正本は `scripts/native-dependencies.json`。upstream source URL と SHA-256、license 原文名、configure / CMake 引数を固定する。SVT の license は実際の原文に従い `BSD-3-Clause-Clear AND BSD-2-Clause` とし、PATENTS もコピーする。
- `scripts/build_ffmpeg_lgpl.py` は shared のみをビルドする。GPL / nonfree / autodetect / network を無効にし、libsvtav1 と libdav1d を明示有効にする。FFmpeg native AV1 decoder は hardware を要求するため、AV1 software roundtrip 用に BSD-2-Clause の dav1d を同梱する（[upstream AV1 decoder](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/libavcodec/av1dec.c)）。macOS は VideoToolbox / AudioToolbox を明示有効にする。全 libav の loaded license / configuration、ABI major、AV1 / ProRes の存在を検証して receipt を書く。配布時の共有ライブラリ・license・source manifest と、開発用 system FFmpeg を混同しない。
- Rust の既存 FFmpeg binding の版追従に依存せず、pkg-config headers に対してコンパイルする小さな C shim を採用する。構造体アクセスは shim 内だけ、Rust 側は opaque handle と plain frame のみ。`cc` による静的リンクは本プロジェクトの shim だけであり、FFmpeg は `dlopen` / `dlsym` による共有ライブラリの動的リンクとする。
- 実行時の `KRONELLO_FFMPEG_LIB_DIR` は **lib ディレクトリそのもの**を指定する。指定先がない、symbol がない、ABI がコンパイル時 headers と違う場合は `FFMPEG_UNAVAILABLE`。暗黙に既定 directory へ戻さない。override の有無・canonical directory・全 library の version / license / configuration を `capabilities.get` で返す。
- release 対応版は単一 major の FFmpeg 9。開発・Ubuntu CI の system headers を使う build はその headers の ABI を要求し、別 major の差し替えは認めない。Ubuntu の FFmpeg / libav*-dev の GPL 構成は開発専用として capabilities に明記し、配布検証の成功とは扱わない。
- 素材解決は毎回全 SHA-256 を検証する。size / mtime cache は導入しない。同サイズ・同更新時刻の内容変更を見逃さないことを優先する。一つの decode session の開始前に照合し、session 内の frame ごとの再照合は呼出側が要求した場合に行う。ジョブ開始・再開では必ず再解決する。
- 相対 locator が存在して hash が違う場合、絶対 locator の一致ファイルへ自動で移らず `ASSET_HASH_MISMATCH`。相対ファイルが欠落した場合だけ絶対 locator を試す。relink は明示した directory を lexical 順に探索し、symlink を辿らず hash 一致だけを更新する。
- collect は stage に素材と store による `.kronello` の複製を書き、コピー後の hash も確認する。出力名を `create_dir` で予約し、既存 directory は拒否する。locator の absolute は削除し、project 基準の relative のみとする。
- decode は software 経路を明示選択する。各要求で stream start に実 seek・flush して presentation 順に decode forward する。次 PTS までを `[pts, next_pts)`、最終 frame は明示 duration までとし、duration 不明時は失敗する。GOP 長や平均 fps を仮定しない。効率化は後続実装で同じ exact seek テストを維持して行う。
- decode frame は native plane の alignment 1 の packed bytes と source color tags を返し、10-bit PQ / HLG を 8-bit に落とさない。色変換・線形化・premultiply は COLOR-001 で扱う。
- encode の入力は **opaque、straight RGBA8、BT.709 encoded RGB**。HDR / linear / 半透明を黙って扱わない。software AV1 は libsvtav1（検出先が libaom なら libaom-av1）、ProRes は prores_ks。H.264 / HEVC は VideoToolbox のみ、`allow_sw=0`。VideoToolbox は upstream の `AV_CODEC_CAP_HYBRID` 登録であるため、codec の hardware-capable 検出は HARDWARE / HYBRID の両方を含める（[upstream VideoToolbox encoder](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/libavcodec/videotoolboxenc.c)）。open failure は codec 名・pixel format・寸法・time_base・FFmpeg error code / string を保持する。未検出・open 失敗は `ENCODER_UNAVAILABLE`。公開 API は enum と plain pixel のみで任意 codec 名 / FFmpeg 引数を受け取らない。
- 出力は ProRes を MOV、それ以外を MP4 とし、track timescale は入力 time_base の分母。入力 PTS は整数 ticks のみを受理し、encoder / muxer へ rational のまま渡す。AV1 の Matroska millisecond 丸めを避ける。出力は stage file の no-clobber publication とする。
- path report は codec 名、software / hardware、入出力 pixel format と転送経路を返す。CPU memcpy と色変換の入出力 payload bytes、hardware upload の logical native payload bytes を分ける。GPU driver 内部の実転送量や同期時間の実測ではなく、GPU 常駐を保証する報告ではない。

## 検証と影響

- CFR 全 5 rate・VFR・LGPL native MPEG-4 B-frame fixture を forward / backward / repeated seek し、端点・内部時刻・終端・drain を比較する。
- hash mismatch / missing / relink / portable collect / revision conflict を通常テストで確認する。
- release の合否は LGPL 同梱 build の capabilities・codec roundtrip による。system GPL build の成功をその代替にしない。
- hardware host の acceptance は `hardware_roundtrip` example で明示実行する。software decode から GPU resident render への統合は別タスクであり、MEDIA-001 の完了をその保証としない。

## 関連

- [12 プラットフォームと依存](../architecture/12-platform-dependencies.md)
- [MEDIA-001 の検証](../testing/media-001.md)
