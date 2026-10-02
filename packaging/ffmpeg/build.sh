#!/usr/bin/env bash
# Builds the decode-only static FFmpeg that vScribe links into its binary.
# Usage: packaging/ffmpeg/build.sh [prefix]   (default prefix: packaging/ffmpeg/out)
# On Windows, run from an MSYS2 shell that inherits an MSVC developer environment.
set -euo pipefail

version=8.1.3
here=$(cd "$(dirname "$0")" && pwd)
prefix=${1:-$here/out}
work=$here/work

if [ "$(cat "$prefix/VERSION" 2>/dev/null)" = "$version" ]; then
  echo "FFmpeg $version is already built in $prefix"
  exit 0
fi

mkdir -p "$work"
cd "$work"
if [ ! -d "ffmpeg-$version" ]; then
  curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$version.tar.xz" -o "ffmpeg-$version.tar.xz"
  tar xJf "ffmpeg-$version.tar.xz"
fi
cd "ffmpeg-$version"

toolchain=
runtime=
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    toolchain=--toolchain=msvc
    runtime=--extra-cflags=-MT
    prefix=$(cygpath -m "$prefix")
    ;;
esac

./configure \
  --prefix="$prefix" \
  ${toolchain:+"$toolchain"} ${runtime:+"$runtime"} \
  --enable-static --disable-shared --enable-pic \
  --disable-everything --disable-autodetect --disable-programs --disable-doc --disable-network \
  --disable-avdevice --disable-avfilter --disable-swscale --disable-x86asm \
  --enable-protocol=file \
  --enable-demuxer=aac,aiff,amr,caf,flac,matroska,mov,mp3,ogg,w64,wav \
  --enable-parser=aac,aac_latm,amr,flac,mpegaudio,opus,vorbis \
  --enable-decoder=aac,aac_latm,adpcm_ima_wav,adpcm_ms,alac,amrnb,amrwb,flac,mp1,mp2,mp3,mp3float,opus,vorbis \
  --enable-decoder=pcm_alaw,pcm_mulaw,pcm_u8,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be \
  --enable-decoder=pcm_f32le,pcm_f32be,pcm_f64le,pcm_f64be \
  --enable-swresample

make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)"
make install

echo "$version" > "$prefix/VERSION"
