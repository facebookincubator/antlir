#!/bin/bash -uex
set -o pipefail
test $# -eq 2
tarball_name=$1  # Exercises `generator_args`
out_dir=$2  # Provided per the tarball generator contract
tmp_dir=$(mktemp -d)
trap 'rm -rf \"$tmp_dir\"' EXIT

# Make an inner "$tmp_dir/d" so we don't have to change the permissions on
# the outer "$tmp_dir" (`mktemp` makes those restrictive by default.)
mkdir "$tmp_dir"/d/
touch "$tmp_dir"/d/hello_world

# Use deterministic options to create this tarball, as per
# reproducible-builds.org.  This ensures its hash is stable.
tar --sort=name --mtime=2018-01-01 --owner=0 --group=0 --numeric-owner \
  -C "$tmp_dir/d" -cf "$out_dir/$tarball_name" .

echo "$tarball_name"  # Required by the tarball generator contract
