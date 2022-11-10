---
id: installing
title: Installation
---

## Dependencies

Antlir has a relatively small set of dependencies required on the build host.

- `buck` - Antlir bundles a script to download buck in `tools/buck`
- `java-11-openjdk` - for `buck`
- `python3 >= 3.7`
- `systemd-nspawn` - usually provided by `systemd` or `systemd-container`
- `btrfs-progs`
- `libcap-ng-devel`
- `gcc` or `clang`
- `rpm2cpio`
- `zstd`
- [`watchman`](https://facebook.github.io/watchman/docs/install.html) - optional but recommended for faster builds
- a working `cgroupv2` setup (first introduced in the 4.5 kernel and already enabled on many modern distros)

### Additional dependencies for re-bootstrapping

This should rarely be required as the build appliance shipped with Antlir can rebuild itself. However, to rebuild the build appliance using only the host system, Antlir requires `dnf` and/or `yum` to be installed on the host.

### Direnv

Antlir comes with a `.envrc` for use with [`direnv`](https://direnv.net/) that makes some Antlir-related commands easier to use.

Currently, it simply adds `tools/` to your `$PATH` when entering the `antlir/` repo directory, which allows you to transparently use the copy of `buck` that Antlir is distributed with. In the future this may be expanded to offer more aliases.

## Git submodules

After initial `git clone` the repo lacks submodule contents. You need to fetch them (after `cd` to repo root) with:

```
$ git submodule update --init
```

## Fetch remote artifacts

Antlir downloads some dependencies from the internet. It is advised to download these with `buck` before attempting to build any images:

```
buck fetch //...
```

## Test your installation

A quick test to confirm that your environment is setup correctly:

```
buck run //images/appliance:rc-build-appliance=container
```

This will give you a shell in the container that Antlir uses for container build operations. If this works you should be ready to build some images by going back to the [Getting Started](getting_started.md) page.

### Troubleshooting

#### cgroupv2

The most common case for the above failing is an issue with your host's cgroups setup. Antlir requires cgroupv2 to be enabled. Many recent distros already have cgroupv2 enabled, and others should have guides to do so. Usually this is just setting `systemd.unified_cgroup_hierarchy=1` on your kernel cmdline for `systemd`-based systems so that `systemd` will mount cgroupv2 at `/sys/fs/cgroup`.

#### Running BUCK as root

It is not advised to run BUCK as a root user. If this is done, then mostly `buck clean` has to to be called.

However, some invocations when running `buck {build, test, run}` requires the user to have sudo persmissions without having to enter the password manually. This can be done by [adding the user to the sudoers file](https://linuxize.com/post/how-to-add-user-to-sudoers-in-ubuntu/#adding-user-to-the-sudoers-file).
