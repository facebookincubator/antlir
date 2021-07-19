#!/usr/bin/env bash

# usage: build.sh qemu-version pixman-version licap_ng-version
set -e
pushd /_temp_qemu

# pixman
tar -xzf pixman.tar.gz --skip-old-files
pushd pixman-pixman-$2
./autogen.sh
export CFLAGS="-fPIE"
./configure --enable-static=yes --prefix=/output
make
make install
popd

# libcap-ng
tar -xzf libcap-ng.tar.gz --skip-old-files
pushd libcap-ng-$3
./autogen.sh
./configure --enable-static=yes --prefix=/output
make
make install
popd

# qemu
tar -xJf source.tar.xz --skip-old-files
pushd qemu-$1
export LDFLAGS="-L/output/lib/"
export CFLAGS="-I/output/include/"
export PKG_CONFIG_PATH="/output/lib/pkgconfig/:/usr/lib64/pkgconfig/"
patch meson.build /_temp_qemu/meson.build.patch
./configure \
  --prefix=/output \
  --static \
  --target-list=x86_64-softmmu \
  --disable-slirp \
  --enable-virtfs
make
make install
popd
