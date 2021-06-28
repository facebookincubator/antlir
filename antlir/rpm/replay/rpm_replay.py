# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See `replay_rpms_and_compiler_items()`"
import itertools
import logging
import pwd
from contextlib import ExitStack, contextmanager
from typing import Callable, Iterator, Mapping, Sequence

from antlir.common import get_logger
from antlir.compiler.compiler import (
    compile_items_to_subvol,
    ImageItem,
    LayerOpts,
)
from antlir.compiler.items.make_subvol import ParentLayerItem
from antlir.fs_utils import temp_dir
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.rpm.replay.subvol_rpm_compare import (
    NEVRA,
    RpmDiff,
    SubvolsToCompare,
)
from antlir.subvol_utils import Subvol, TempSubvolumes

log = get_logger()

ReplayItemsGenerator = Callable[[ExitStack, LayerOpts], Iterator[ImageItem]]

# Should follow https://fburl.com/diffusion/somgsffm
_RPM_INSTALL_CMD = [
    "/bin/rpm",
    "-i",
    "--nofiledigest",
    "--nodigest",
    "--nosignature",
]


def _install_rpms_into_subvol(
    *,
    ba_subvol: Subvol,
    install_subvol: Subvol,
    rpms_in_order: Sequence[NEVRA],
    rpm_download_subvol: Subvol,
):
    """
    Use `_RPM_INSTALL_CMD` to install the specified RPMs, in order, from
    `rpm_download_subvol`, using into `install_subvol`.
    """
    with temp_dir() as dev_dir:
        dev_files = ["null", "zero"]
        for f in dev_files:
            (dev_dir / f).touch()  # mountpoints used below
        opts = new_nspawn_opts(
            bindmount_ro=[
                (rpm_download_subvol.path(), "/d"),
                # Scriptlets need a basic /dev setup, and Antlir's
                # yum_dnf_from_snapshot protections do not apply here
                (dev_dir, "/i/dev"),
                *[(f"/dev/{f}", f"/i/dev/{f}") for f in dev_files],
            ],
            bindmount_rw=[(install_subvol.path(), "/i")],
            user=pwd.getpwnam("root"),
            cmd=[
                *_RPM_INSTALL_CMD,
                "--root=/i",
                *[f"/d/{r.download_path()}" for r in rpms_in_order],
            ],
            layer=ba_subvol,
        )
        # Deliberately run with no repo-server, we only need the local RPMs
        run_nspawn(opts, PopenArgs())


@contextmanager
def replay_rpms_and_compiler_items(
    *,
    # The first 2 args come from `subvol_rpm_compare_and_download()`
    rpm_diff: RpmDiff,
    rpm_download_subvol: Subvol,
    # These are needed for the replay logic.
    subvols: SubvolsToCompare,
    flavor: str,
    artifacts_may_require_repo: bool,
    target_to_path: Mapping[str, str],
    gen_replay_items: ReplayItemsGenerator,
) -> Iterator[Subvol]:
    """
    Chain this after `subvol_rpm_compare_and_download()`.

    Replays `rpm_diff` (using RPMs from `rpm_download_subvol`) together with
    the Antlir compiler `ImageItems` from `gen_replay_items()`, on top
    of `subvols.root`, with the intention of reproducing `subvols.leaf`.
    """
    layer_opts = LayerOpts(
        artifacts_may_require_repo=artifacts_may_require_repo,
        build_appliance=subvols.ba,
        layer_target="unimportant",
        rpm_installer=subvols.rpm_installer,
        # pyre-fixme[16]: `SubvolsToCompare` has no attribute `flavor`.
        flavor=subvols.flavor,
        rpm_repo_snapshot=subvols.rpm_repo_snapshot,
        target_to_path=target_to_path,
        subvolumes_dir=None,
        version_set_override=None,
        debug=log.isEnabledFor(logging.DEBUG),
    )
    with TempSubvolumes() as tmp_subvols, ExitStack() as exit_stack:
        if rpm_diff.removed:
            raise NotImplementedError(
                f"Incremental RPM replay cannot remove RPMs: {rpm_diff.removed}"
            )

        # Replay the non-RPM compiler items on top of `subvols.root`.
        install_subvol = tmp_subvols.caller_will_create("rpm_replay")
        compile_items_to_subvol(
            exit_stack=exit_stack,
            subvol=install_subvol,
            layer_opts=layer_opts,
            iter_items=itertools.chain(
                [
                    ParentLayerItem(
                        from_target="//FAKE:ordered_nevras_for_rpm_replay",
                        subvol=subvols.root,
                    )
                ],
                gen_replay_items(exit_stack, layer_opts),
            ),
        )

        # Install the specified RPMs in the specified order via `/bin/rpm`
        _install_rpms_into_subvol(
            ba_subvol=subvols.ba,
            install_subvol=install_subvol,
            rpms_in_order=rpm_diff.added_in_order,
            rpm_download_subvol=rpm_download_subvol,
        )

        yield install_subvol
