#!/usr/bin/env bash

# usage: build.sh qemu-version pixman-version licap_ng-version
set -e

# pixman
tar -xzf pixman.tar.gz --skip-old-files
cd pixman-pixman-$2
./autogen.sh
mkdir -p /_temp_qemu/pixman
export CFLAGS="-fPIE"
./configure --enable-static=yes --prefix=/_temp_qemu/pixman
make
make install
cd ..

# libcap-ng
tar -xzf libcap-ng.tar.gz --skip-old-files
cd libcap-ng-$3
./autogen.sh
mkdir -p /_temp_qemu/libcap-ng
./configure --enable-static=yes --prefix=/_temp_qemu/libcap-ng
make
make install
cd ..

# qemu
tar -xJf source.tar.xz --skip-old-files
cd qemu-$1
export LDFLAGS="-L/_temp_qemu/pixman/lib/ -L/_temp_qemu/libcap-ng/lib/"
export CFLAGS="-I/_temp_qemu/pixman/include/ -I/_temp_qemu/libcap-ng/include/"
export PKG_CONFIG_PATH="/_temp_qemu/pixman/lib/pkgconfig/:/_temp_qemu/libcap-ng/lib/pkgconfig/:/usr/lib64/pkgconfig/"
patch meson.build /_temp_qemu/meson.build.patch
./configure \
  --prefix=/output/qemu \
  --static \
  --target-list=x86_64-softmmu \
  --disable-slirp \
  --enable-virtfs
make
make install
cd ..
