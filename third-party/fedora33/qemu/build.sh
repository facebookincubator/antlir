#!/usr/bin/env bash

# usage: build.sh qemu-version pixman-version
set -e

# pixman
tar -xzf pixman.tar.gz --skip-old-files
cd pixman-pixman-$2
./autogen.sh
export CFLAGS="$CFLAGS -fPIE"
mkdir -p /_temp_qemu/pixman
./configure --prefix=/_temp_qemu/pixman --enable-static=yes
make
make install
cd ..

# qemu
tar -xJf source.tar.xz --skip-old-files
cd qemu-$1
export LDFLAGS="$LDFLAGS -L/_temp_qemu/pixman/lib/"
export CFLAGS="$CFLAGS -I/_temp_qemu/pixman/include/"
patch meson.build /_temp_qemu/meson.build.patch
patch /usr/lib64/pkgconfig/libcap-ng.pc /_temp_qemu/libcap-ng.pc.patch
mkdir -p /output/qemu
./configure \
  --prefix=/output/qemu \
  --static \
  --target-list=x86_64-softmmu \
  --disable-slirp \
  --enable-virtfs
make
make install
cd ..
