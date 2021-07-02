#!/usr/bin/env bash

# usage: build.sh qemu-version pixman-version
set -e

# pixman
tar -xzf pixman.tar.gz
cd pixman-pixman-$2
./autogen.sh
export CFLAGS="$CFLAGS -fPIE"
mkdir /_temp_qemu/pixman
./configure --enable-static=yes --prefix=/_temp_qemu/pixman
make
make install
cd ..

# qemu
tar -xJf source.tar.xz
cd qemu-$1
export LDFLAGS="$LDFLAGS -L/_temp_qemu/pixman/lib/"
export CFLAGS="$CFLAGS -I/_temp_qemu/pixman/include/"
mkdir /output/qemu
./configure --target-list=x86_64-softmmu --disable-slirp --static --prefix=/output/qemu
make
make install
cd ..
