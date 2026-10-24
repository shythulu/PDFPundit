#!/usr/bin/env bash
# Stage 4 verifier: build the two WP-3.1 pins that conda-forge does not ship (2026-10-05):
# MuPDF 1.28.5 (conda-forge newest 1.28.0) and Poppler 26.10.0 (conda-forge newest 26.09.0).
# Everything goes under $S; no apt, no sudo; nice -n 10, -j2; each build capped at 20 minutes.
#   S=<scratch dir> [MUPDF_VER=1.28.0] bash build_pins.sh mupdf|poppler
# Poppler is configured against the conda-forge scratch env $S/cenv (it already holds Poppler 26.09.0's
# dependencies: freetype, fontconfig, cairo, libjpeg-turbo, openjpeg, lcms2, libpng, libtiff, zlib).
set -euo pipefail
: "${S:?set S to the scratch directory}"
mkdir -p "$S/src" "$S/pins"
case "$1" in
  mupdf)
    V=${MUPDF_VER:-1.28.5}   # 1.28.0 too: a same-recipe source build separates version from build differences
    cd "$S/src"
    [ -f mupdf-$V-source.tar.gz ] || curl -sSfL -o mupdf-$V-source.tar.gz https://mupdf.com/downloads/archive/mupdf-$V-source.tar.gz
    sha256sum mupdf-$V-source.tar.gz
    rm -rf mupdf-$V-source && tar xzf mupdf-$V-source.tar.gz
    cd mupdf-$V-source
    timeout 1200 nice -n 10 make -j2 HAVE_X11=no HAVE_GLUT=no HAVE_CURL=no build=release prefix="$S/pins/mupdf-$V" tools
    timeout 300 nice -n 10 make -j2 HAVE_X11=no HAVE_GLUT=no HAVE_CURL=no build=release prefix="$S/pins/mupdf-$V" install-tools 2>/dev/null \
      || { mkdir -p "$S/pins/mupdf-$V/bin"; cp build/release/mutool "$S/pins/mupdf-$V/bin/"; }
    "$S/pins/mupdf-$V/bin/mutool" -v 2>&1 | head -1 || true ;;
  poppler)
    cd "$S/src"
    [ -f poppler-26.10.0.tar.xz ] || curl -sSfL -o poppler-26.10.0.tar.xz https://poppler.freedesktop.org/poppler-26.10.0.tar.xz
    sha256sum poppler-26.10.0.tar.xz
    rm -rf poppler-26.10.0 && tar xJf poppler-26.10.0.tar.xz
    cd poppler-26.10.0 && mkdir -p build && cd build
    PKG_CONFIG_PATH="$S/cenv/lib/pkgconfig:$S/cenv/share/pkgconfig" timeout 300 nice -n 10 cmake .. -G Ninja \
      -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$S/pins/poppler-26.10.0" -DCMAKE_PREFIX_PATH="$S/cenv" \
      -DCMAKE_INSTALL_RPATH="$S/pins/poppler-26.10.0/lib;$S/cenv/lib" -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
      -DENABLE_QT5=OFF -DENABLE_QT6=OFF -DENABLE_GLIB=OFF -DENABLE_GOBJECT_INTROSPECTION=OFF -DENABLE_CPP=OFF \
      -DENABLE_GPGME=OFF -DENABLE_NSS3=OFF -DENABLE_BOOST=OFF -DENABLE_LIBCURL=OFF -DENABLE_LIBTIFF=OFF \
      -DBUILD_GTK_TESTS=OFF -DBUILD_QT5_TESTS=OFF -DBUILD_QT6_TESTS=OFF -DBUILD_CPP_TESTS=OFF -DBUILD_MANUAL_TESTS=OFF \
      -DENABLE_UTILS=ON -DRUN_GPERF_IF_PRESENT=OFF -DENABLE_HARFBUZZ=OFF
    timeout 1200 nice -n 10 ninja -j2
    timeout 300 nice -n 10 ninja install >/dev/null
    "$S/pins/poppler-26.10.0/bin/pdftotext" -v 2>&1 | head -1 || true ;;
esac
