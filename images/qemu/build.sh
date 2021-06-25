#!/usr/bin/env

# usage: build.sh path/to/qemu-sources.tar.xz version-number

tar -xvJf $1
cd qemu-$2
./configure # --target-list=x86_64-softmmu --disable-slirp --static --enable-kvm
make
# make install
