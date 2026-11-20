#!/usr/bin/env bash
set -euo pipefail
ndk_local=$1
make_local=$2
export PATH=/usr/bin:/bin:$PATH
./configure --target-os=android --arch=aarch64 --enable-cross-compile \
    --host-cc="$ndk_local/bin/clang.exe" --host-cflags=--target=x86_64-pc-windows-msvc --host-ldflags="--target=x86_64-pc-windows-msvc -fuse-ld=lld" --cc="$ndk_local/bin/clang.exe" --ar="$ndk_local/bin/llvm-ar.exe" \
    --nm="$ndk_local/bin/llvm-nm.exe" --ranlib="$ndk_local/bin/llvm-ranlib.exe" \
    --strip="$ndk_local/bin/llvm-strip.exe" \
    --extra-cflags=--target=aarch64-linux-android26 \
    --extra-ldflags='--target=aarch64-linux-android26 -Wl,-z,max-page-size=16384' \
    --disable-autodetect --disable-everything --disable-doc --disable-debug \
    --disable-ffplay --disable-ffprobe --disable-network --enable-small \
    --enable-ffmpeg --enable-avformat --enable-avcodec --enable-avfilter --enable-swscale --enable-swresample \
    --enable-demuxer=bink --enable-decoder=bink,binkaudio_rdft,binkaudio_dct \
    --enable-muxer=rawvideo,pcm_s16le --enable-encoder=rawvideo,pcm_s16le \
    --enable-protocol=file,pipe --enable-filter=join,aformat,aresample,format,scale,trim,atrim,null,anull
"$make_local" -j 8 ffmpeg
