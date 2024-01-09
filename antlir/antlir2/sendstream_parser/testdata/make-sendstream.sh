#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

out="$(realpath "$2")"

pushd "$1"

btrfs subvolume delete demo demo-undo || true

btrfs subvolume create demo
pushd demo
mkdir hello
echo "Hello world!" > hello/msg
chmod 0400 hello/msg
setfattr -n user.antlir.demo -v 'lorem ipsum' hello/msg
chown root:root hello/msg
mkfifo myfifo
ln -s hello/msg hello/msg-sym
ln hello/msg hello/msg-hard
touch to-be-deleted
mkdir dir-to-be-deleted
# larger example file that will not fit in the inode struct so will exercise
# reflinking
lorem="Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum."
echo "$lorem" > hello/lorem
set +x
for _i in {1..500}
do
    echo "$lorem" >> hello/lorem
done
set -x
cp --reflink=always hello/lorem hello/lorem-reflinked
truncate -s100G huge-empty-file
mknod null c 1 3
python3 -c "import socket as s; sock = s.socket(s.AF_UNIX); sock.bind('socket-node.sock')"

popd

btrfs subvolume snapshot demo demo-undo
btrfs property set demo ro true

pushd demo-undo

rm to-be-deleted
rmdir dir-to-be-deleted
setfattr --remove user.antlir.demo hello/msg
echo "Goodbye!" > hello/msg

popd

btrfs property set demo-undo ro true

btrfs send demo -f "$out.1"
btrfs send -p demo demo-undo -f "$out.2"
cat "$out.1" "$out.2" > "$out"
rm "$out.1" "$out.2"

popd
