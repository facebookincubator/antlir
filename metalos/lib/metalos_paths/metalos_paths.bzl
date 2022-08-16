# Copyright (c) Meta Platforms, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

# All special-meaning MetalOS on-disk paths in one place to prevent copy-pasta
# from proliferating across many different MetalOS libraries.
# Keep this organized along the existing lines, or refactor as needed.

load("//antlir/bzl:structs.bzl", "structs")

metalos_paths = struct(
    control = "/run/fs/control",
    # Subvolumes to store the readonly images
    images = struct(
        base = "/run/fs/control/image",
        **{
            kind: "/run/fs/control/image/{}".format(kind)
            for kind in (
                "initrd",
                "kernel",
                "rootfs",
                "service",
                "service-config-generator",
            )
        }
    ),
    # Various subvolumes to store runtime state of the rootfs and native
    # services
    runtime = struct(
        base = "/run/fs/control/run",
        **{
            sub: "/run/fs/control/run/{}".format(sub)
            for sub in (
                # This directory stores all the current and previously built
                # boot environments each has the unique uuid of the boot and is
                # a snapshot of that boots rootfs with all the necessary
                # packages mounted into it and the generators run inside of it.
                "boot",
                "cache",
                "kernel",
                "logs",
                "runtime",
                # Temporary storage space, but for things that need to be on BTRFS. For
                # example, sendstreams are temporarily received here before being moved to
                # their actual destination.
                "scratch",
                # Persistent directories for Native Services
                "state",
                "service-roots",
                "swap",
            )
        }
    ),
    # MetalOS-managed state (for core tooling and anything that core tooling
    # provides to user code)
    core_state = struct(
        base = "/run/fs/control/run/state",
        # Root directory for MetalOS-internal persistent state. Never exposed
        # directly to user code.
        metalos = "/run/fs/control/run/state/metalos",
    ),
    # MetalOS-managed directory for persistent storage of things like
    # certificates. User code should not assume any of these paths are stable,
    # and MetalOS will manage bindmounts/symlinks as required.
    managed_state = struct(
        base = "/run/fs/control/run/state/persistent",
        certs = "/run/fs/control/run/state/persistent/certs",
    ),
)

def dirs_to_create():
    paths = _flat_struct_values(metalos_paths)
    paths = {p: _stat_for_dir(p) for p in paths}

    # we don't need to create the root of the hierarchy, and in fact this will
    # cause tons of failures in downstream macros that are easier to just take
    # care of now
    paths.pop(metalos_paths.control)
    return paths

def dir_attrs():
    paths = _flat_struct_values(metalos_paths)
    return {p: _attrs_for_dir(p) for p in paths if len(_attrs_for_dir(p)) > 0}

def _attrs_for_dir(path):
    attr = ""
    if path == metalos_paths.runtime.swap:
        # Disable Copy-on-Write for the swap subvol to avoid swapfiles fragmentation
        # https://man7.org/linux/man-pages/man1/chattr.1.html
        attr = "+C"
    return attr

def _stat_for_dir(path):
    mode = 0o555
    if path == metalos_paths.managed_state.certs:
        mode = 0o400
    return struct(
        user = "root",
        group = "root",
        mode = mode,
    )

def _flat_struct_values(strct):
    flat = []
    for v in structs.to_dict(strct).values():
        if structs.is_struct(v):
            flat.extend(_flat_struct_values(v))
        else:
            flat.append(v)
    return flat

def _rust_module(strct, indent = 0):
    mod = ["{"]
    for k, v in structs.to_dict(strct).items():
        k = k.replace("-", "_")
        if structs.is_struct(v):
            mod.append("  pub mod {}".format(k))
            mod.append(_rust_module(v, indent + 1))
        else:
            mod.append("  pub fn {}() -> &'static ::std::path::Path {{".format(k))
            mod.append("    ::std::path::Path::new(\"{}\")".format(v))
            mod.append("  }")
    mod.append("}")
    mod = ["  " * indent + line for line in mod]
    return "\n".join(mod)

def gen_rust_module():
    gen_src = _rust_module(metalos_paths)
    docblock = """
//! Various MetalOS on-disk paths in one place to prevent copy-pasta from
//! proliferating across many different MetalOS libraries.
//! See metalos/lib/metalos_paths:metalos_paths.bzl for more descriptions about
//! what each of these paths are used for
"""
    return "{}\nmod gen {}\npub use gen::*;".format(docblock, gen_src)

def _oct(x):
    if x > 0o777:
        fail("octal mode must be less than 0o777")
    d1 = x // 64
    x = x % 64
    d2 = x // 8
    d3 = x % 8
    return d3 + d2 * 10 + d1 * 100

def gen_tmpfiles():
    conf = []
    for path, stat in dirs_to_create().items():
        # For all paths, create them as subvolumes
        # https://www.freedesktop.org/software/systemd/man/tmpfiles.d.html#v
        # v     /subvolume-or-directory/to/create        mode user group cleanup-age -
        conf.append("v {} 0{} {} {} - -".format(path, _oct(stat.mode), stat.user, stat.group))
    for path, attrs in dir_attrs().items():
        # Specify (non-recursive) attributes to apply. Assumes path was created above.
        # https://www.freedesktop.org/software/systemd/man/tmpfiles.d.html#h
        conf.append("h {} - - - - {}".format(path, attrs))

    # note: the sorted here is very important so that the subvolume tree gets
    # created in the right order
    return "\n".join(sorted(conf))
