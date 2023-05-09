# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See `replay_rpms_and_compiler_items()`"
import pickle
import pwd
from contextlib import contextmanager
from typing import Any, Callable, Iterator, List, Sequence, Tuple

from antlir.common import get_logger, not_none
from antlir.compiler.dep_graph import ImageItem
from antlir.compiler.helpers import get_compiler_nspawn_opts
from antlir.compiler.items.common import LayerOpts
from antlir.compiler.items.make_subvol import ParentLayerItem
from antlir.fs_utils import Path, temp_dir
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.rpm.replay.subvol_rpm_compare import NEVRA, RpmDiff
from antlir.subvol_utils import Subvol, TempSubvolumes

log = get_logger()

ReplayItemsGenerator = Callable[[LayerOpts], Iterator[ImageItem]]

# Should follow https://fburl.com/diffusion/somgsffm
_RPM_INSTALL_CMD = [
    "/bin/rpm",
    "-i",
    "--nofiledigest",
    "--nodigest",
    "--nosignature",
]


def filter_features_to_replay(
    features_to_replay: List[Tuple[str, str, Any]]
) -> List[Tuple[str, str, Any]]:
    return [
        feature
        for feature in features_to_replay
        if feature[0]
        not in {
            "parent_layer",  # Layering shouldn't affect contents
            # Captured by `install_rpm_names`, but we don't
            # want to replay these with `RpmActionItem`, the
            # whole point is to use `rpm -i` instead.
            "rpms",
            "layer_from_package",  # `packaged_root`
        }
    ]


def _install_rpms_into_subvol(
    *,
    ba_subvol: Subvol,
    install_subvol: Subvol,
    rpms_in_order: Sequence[NEVRA],
    rpm_download_subvol: Subvol,
) -> None:
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
            # We strip env vars upon container entry, and the default locale
            # is `C`.  However, `dnf` internally falls back to `C.UTF-8`, so
            # scriptlets act on the filesystem in that locale (for example,
            # `/root/.viminfo` as written by `vim-enhanced`, stores the
            # encoding).  Set this to ensure we match `dnf` behavior.
            setenv=["LC_ALL=C.UTF-8"],
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
    # The next 3 are needed for the replay logic.
    root: Subvol,
    layer_opts: LayerOpts,
    gen_replay_items: ReplayItemsGenerator,
    compile_items_to_subvol_bin_path: Path,
) -> Iterator[Subvol]:
    """
    Chain this after `subvol_rpm_compare_and_download()`.

    Replays `rpm_diff` (using RPMs from `rpm_download_subvol`) together with
    the Antlir compiler `ImageItems` from `gen_replay_items()`, on top of
    `root`, with the intention of reproducing the `leaf` that was given to
    `subvol_rpm_compare_and_download()`.
    """
    with TempSubvolumes() as tmp_subvols:
        if rpm_diff.removed:  # pragma: no cover
            raise NotImplementedError(
                f"Incremental RPM replay cannot remove RPMs: {rpm_diff.removed}"
            )

        # Replay the non-RPM compiler items on top of `root`.
        install_subvol = tmp_subvols.caller_will_create("rpm_replay")
        compile_items_to_subvol_args = {
            "subvol": install_subvol,
            "layer_opts": layer_opts,
            "iter_items": [
                ParentLayerItem(
                    from_target="//FAKE:ordered_nevras_for_rpm_replay",
                    subvol=root,
                ),
                *list(gen_replay_items(layer_opts)),
            ],
        }
        run_nspawn(
            get_compiler_nspawn_opts(
                cmd=[
                    compile_items_to_subvol_bin_path,
                    # use protocol 0 to avoid generating any null bytes
                    pickle.dumps(compile_items_to_subvol_args, 0),
                ],
                build_appliance=not_none(layer_opts.build_appliance),
                ro_bindmounts=[
                    (compile_items_to_subvol_bin_path, compile_items_to_subvol_bin_path)
                ],
            ),
            PopenArgs(),
        )

        # Install the specified RPMs in the specified order via `/bin/rpm`
        _install_rpms_into_subvol(
            ba_subvol=not_none(layer_opts.build_appliance),
            install_subvol=install_subvol,
            rpms_in_order=rpm_diff.added_in_order,
            rpm_download_subvol=rpm_download_subvol,
        )

        yield install_subvol
