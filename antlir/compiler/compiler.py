#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is normally invoked by the `image_layer` Buck macro converter.

This compiler builds a btrfs subvolume in
  <--subvolumes-dir>/<--subvolume-rel-path>

To do so, it parses `--child-feature-json` and the `--child-dependencies`
that referred therein, creates `ImageItems`, sorts them in dependency order,
and invokes `.build()` to apply each item to actually construct the subvol.
"""

import cProfile
import os
import stat
import sys
import time
import uuid
from contextlib import nullcontext
from subprocess import CalledProcessError
from typing import List, Optional

from antlir.bzl.constants import flavor_config_t
from antlir.cli import normalize_buck_path
from antlir.compiler.helpers import compile_items_to_subvol, get_compiler_nspawn_opts
from antlir.compiler.items.common import LayerOpts
from antlir.compiler.items.rpm_action import RpmActionItem
from antlir.compiler.items_for_features import gen_items_for_features
from antlir.compiler.rust.compiler import Args, parse_args
from antlir.compiler.subvolume_on_disk import SubvolumeOnDisk
from antlir.config import repo_config
from antlir.errors import AntlirError
from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import META_FLAVOR_FILE, Path
from antlir.nspawn_in_subvol.args import NspawnPluginArgs, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.nspawn_in_subvol.plugins.repo_plugins import repo_nspawn_plugins
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.subvol_utils import Subvol


def get_parent_layer_flavor_config(parent_layer: Path) -> flavor_config_t:
    parent_layer_subvol = find_built_subvol(parent_layer)
    flavor = parent_layer_subvol.read_path_text(META_FLAVOR_FILE)
    return repo_config().flavor_to_config[flavor]


def construct_profile_filename(layer_target: str, is_nested: bool = True) -> Path:
    return Path(
        layer_target.replace("/", "_") + ("_outer" if not is_nested else "") + ".pstat"
    )


def invoke_compiler_inside_build_appliance(
    *,
    build_appliance: Subvol,
    run_apt_proxy: bool,
    snapshot_dir: Optional[Path],
    args: Args,
    argv: List[str],
):
    rw_bindmounts = []
    if args.profile_dir:
        prof_filename = construct_profile_filename(args.child_layer_target)
        nested_profile_dir = f"/antlir_prof_{uuid.uuid4().hex}"

        # For encapsulation purposes, we make the pstat file ahead of time to
        # restrict the bindmount to be a single controlled file rather than the
        # entire `profile_dir`.
        os.makedirs(args.profile_dir, exist_ok=True)
        (args.profile_dir / prof_filename).touch()

        rw_bindmounts.append(
            (
                args.profile_dir / prof_filename,
                nested_profile_dir / prof_filename,
            )
        )
        argv = argv + ["--profile", nested_profile_dir]

    opts = get_compiler_nspawn_opts(
        cmd=[
            args.compiler_binary,
            "--is-nested",
            *argv,
        ],
        build_appliance=build_appliance,
        rw_bindmounts=rw_bindmounts,
    )
    try:
        run_nspawn(
            opts,
            PopenArgs(),
            plugins=repo_nspawn_plugins(
                opts=opts,
                plugin_args=NspawnPluginArgs(
                    serve_rpm_snapshots=[snapshot_dir] if snapshot_dir else [],
                    # We'll explicitly call the RPM installer wrapper we need.
                    shadow_proxied_binaries=False,
                    run_apt_proxy=run_apt_proxy,
                ),
            ),
        )
    except CalledProcessError as e:  # pragma: no cover
        # If this failed, it's exceedingly unlikely for this backtrace to
        # actually be useful, and instead it just makes it harder to find the
        # "real" backtrace from the internal compiler. However, in the rare
        # chance that it is useful, ANTLIR_DEBUG voids all warranties for a
        # possibly-actually-readable stderr, and will includ the outer backtrace
        # as well as any inner failures
        if args.debug:
            raise e
        sys.exit(e.returncode)


def build_image(args: Args, argv: List[str]) -> SubvolumeOnDisk:
    # We want check the umask since it can affect the result of the
    # `os.access` check for `image.install*` items.  That said, having a
    # umask that denies execute permission to "user" is likely to break this
    # code earlier, since new directories wouldn't be traversible.  At least
    # this check gives a nice error message.
    cur_umask = os.umask(0)
    os.umask(cur_umask)
    assert (
        cur_umask & stat.S_IXUSR == 0
    ), f"Refusing to run with pathological umask 0o{cur_umask:o}"

    subvol = Subvol(args.subvolumes_dir / args.subvolume_rel_path)

    flavor_config = args.flavor_config

    if not flavor_config:
        assert (
            args.parent_layer
        ), "Parent layer must be given if no flavor config is given"
        flavor_config = get_parent_layer_flavor_config(args.parent_layer)

    build_appliance = None
    if flavor_config and flavor_config.build_appliance:
        build_appliance_layer_path = args.targets_and_outputs[
            flavor_config.build_appliance
        ]
        build_appliance = find_built_subvol(build_appliance_layer_path)

    layer_opts = LayerOpts(
        layer_target=args.child_layer_target,
        build_appliance=build_appliance,
        rpm_installer=YumDnf(flavor_config.rpm_installer)
        if flavor_config.rpm_installer
        else None,
        rpm_repo_snapshot=Path(flavor_config.rpm_repo_snapshot)
        if flavor_config.rpm_repo_snapshot
        else None,
        apt_repo_snapshot=flavor_config.apt_repo_snapshot or (),
        artifacts_may_require_repo=args.artifacts_may_require_repo,
        target_to_path=args.targets_and_outputs,
        subvolumes_dir=args.subvolumes_dir,
        version_set_override=args.version_set_override,
        debug=args.debug,
        allowed_host_mount_targets=frozenset(args.allowed_host_mount_target),
        flavor=flavor_config.name,
        # This value should never be inherited from the parent layer
        # as it is generally used to create a new build appliance flavor
        # by force overriding an existing flavor.
        unsafe_bypass_flavor_check=flavor_config.unsafe_bypass_flavor_check,
    )
    layer_items = list(
        gen_items_for_features(
            features_or_paths=[
                normalize_buck_path(output) for output in args.child_feature_json
            ],
            layer_opts=layer_opts,
        )
    )

    # Avoid running the compiler inside of the BA if:
    # 1. The BA isn't set (ie. DO_NOT_USE_BUILD_APPLIANCE). Future: create a
    #    separate lightweight compiler binary for this case.
    # 2. We're already nested inside the BA container.
    # 3. We're compiling a genrule layer. Future: support serving rpm snapshot
    #    in the BA container to remove this restriction.
    if (
        build_appliance
        and not args.is_nested
        and not args.internal_only_is_genrule_layer
    ):
        installs_rpms = any(isinstance(i, RpmActionItem) for i in layer_items)

        invoke_compiler_inside_build_appliance(
            build_appliance=build_appliance,
            snapshot_dir=Path(flavor_config.rpm_repo_snapshot)
            if flavor_config.rpm_repo_snapshot and installs_rpms
            else None,
            run_apt_proxy=bool(
                flavor_config.apt_repo_snapshot
                and (len(flavor_config.apt_repo_snapshot) > 0)
            ),
            args=args,
            argv=argv,
        )
    else:

        # This stack allows build items to hold temporary state on disk.
        compile_items_to_subvol(
            subvol=subvol,
            layer_opts=layer_opts,
            iter_items=layer_items,
        )
        # Build artifacts should never change. Run this BEFORE the
        # exit_stack cleanup to enforce that the cleanup does not
        # touch the image.
        subvol.set_readonly(True)

    try:
        return SubvolumeOnDisk.from_subvolume_path(
            # Converting to a path here does not seem too risky since this
            # class shouldn't have a reason to follow symlinks in the subvol.
            subvol.path(),
            args.subvolumes_dir,
            build_appliance.path() if build_appliance else None,
        )
    # The complexity of covering this is high, but the only thing that can
    # go wrong is a typo in the f-string.
    except Exception as ex:  # pragma: no cover
        raise RuntimeError(f"Serializing subvolume {subvol.path()}") from ex


if __name__ == "__main__":  # pragma: no cover
    from antlir.common import init_logging

    argv = sys.argv[1:]
    args = parse_args(argv)
    init_logging(debug=args.debug)

    start = time.perf_counter()
    with (cProfile.Profile() if args.profile_dir else nullcontext()) as pr:
        try:
            subvol = build_image(args, argv)
            if not args.is_nested:
                subvol.to_json_file(sys.stdout)
        except AntlirError as e:
            if args.debug or e.backtrace_is_interesting:
                raise e
            print(file=sys.stderr)
            print(e, file=sys.stderr)
            assert e.__traceback__ is not None
            print(
                f"  raised at {e.__traceback__.tb_frame.f_code.co_filename}"
                f":{e.__traceback__.tb_lineno}",
                file=sys.stderr,
            )
            sys.exit(1)
    end = time.perf_counter()
    if args.profile_dir:
        assert pr is not None
        filename = construct_profile_filename(
            args.child_layer_target, is_nested=args.is_nested
        )
        os.makedirs(args.profile_dir, exist_ok=True)
        pr.dump_stats(args.profile_dir / filename)
        os.setxattr(
            args.profile_dir / filename,
            "user.antlir.duration",
            f"{end - start}s".encode(),
        )
