#!/usr/bin/env bash

# usage: build.sh qemu-version pixman-version
set -e

# pixman
tar -xzf pixman.tar.gz
cd pixman-pixman-$2
./autogen.sh
export CFLAGS="$CFLAGS -fPIE"
mkdir /_temp_qemu/pixman
./configure --prefix=/_temp_qemu/pixman --enable-static=yes
make
make install
cd ..

# qemu
tar -xJf source.tar.xz
cd qemu-$1
export LDFLAGS="$LDFLAGS -L/_temp_qemu/pixman/lib/"
export CFLAGS="$CFLAGS -I/_temp_qemu/pixman/include/"
export PKG_CONFIG_PATH="$PKG_CONFIG_PATH:/usr/lib:/usr/lib64:/usr/local/lib:/usr/local/lib64"
mkdir /output/qemu
./configure \
  --prefix=/output/qemu \
  --static \
  --target-list=x86_64-softmmu \
  --disable-slirp \
  --enable-virtfs
make
make install
cd ..
