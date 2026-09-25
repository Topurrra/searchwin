# FFmpeg (the FFmpeg pack)

Search doesn't carry FFmpeg. When a person installs the **FFmpeg pack**
(Settings › Packs, or `>install ffmpeg` in the field), Search downloads this
exact build from its publisher, checks its SHA-256, and unpacks only the
files below into `%LOCALAPPDATA%\Search\Packs\ffmpeg\<version>\`. Search runs
`ffmpeg.exe` and `ffprobe.exe` as separate programs, over their command line;
it doesn't link FFmpeg's libraries. Search stays MIT.

| | |
|---|---|
| Software | FFmpeg 8.1.2-50-g1a748fe2cd (release/8.1 branch) |
| Licence | **LGPL-3.0-or-later** (`--enable-version3`); no GPL or non-free parts (`--disable-libx264 --disable-libx265`, no `--enable-gpl`, no `--enable-nonfree`) |
| Exact source | https://github.com/FFmpeg/FFmpeg/commit/1a748fe2cd43e3ead22fafb1b5b7d77f153898a8 |
| Build | BtbN/FFmpeg-Builds, `win64-lgpl-shared-8.1`, release `autobuild-2026-08-31-13-27`: https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-08-31-13-27 (its build scripts at that tag are the build recipe) |
| Download | https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-08-31-13-27/ffmpeg-n8.1.2-50-g1a748fe2cd-win64-lgpl-shared-8.1.zip |
| Size | 70,835,150 bytes (about 143 MB unpacked) |
| SHA-256 | `e9712ffbdb03ef71bbab660c75b835bfe698ef6fad0247c76d8d394a39a3db63` (matches the release's own `checksums.sha256`) |
| Files kept | `bin/ffmpeg.exe`, `bin/ffprobe.exe`, `bin/avcodec-62.dll`, `bin/avdevice-62.dll`, `bin/avfilter-11.dll`, `bin/avformat-62.dll`, `bin/avutil-60.dll`, `bin/swresample-6.dll`, `bin/swscale-9.dll`, `LICENSE.txt` |

The licence text travels with the pack (`LICENSE.txt`, the GNU LGPL v3) and
Settings › Packs links to it, to the build and to the source.

**Codec patents.** The build has no H.264/HEVC encoder of its own (no x264 or
x265). Search encodes H.264 with Windows' own Media Foundation encoder
(`h264_mf`), and AAC with FFmpeg's native encoder.

**Updating.** The manifest (`Search.Kit/Packs/packs.json`) pins this build.
A new build means a new manifest entry (URL, version, size, SHA-256, file
list) in a new Search release. BtbN keeps each month's last autobuild, so
pin a month-end release (`autobuild-YYYY-MM-last`), never `latest`.
