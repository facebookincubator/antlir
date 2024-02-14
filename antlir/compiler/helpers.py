# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import pickle
import pwd
from typing import AnyStr, Iterable, Tuple

from antlir.bzl_const import hostname_for_compiler_in_ba

from antlir.compiler.dep_graph import DependencyGraph, ImageItem
from antlir.compiler.items.common import LayerOpts
from antlir.compiler.items.phases_provide import PhasesProvideItem
from antlir.config import repo_config
from antlir.fs_utils import MehStr
from antlir.nspawn_in_subvol.args import new_nspawn_opts
from antlir.subvol_utils import Subvol


def get_compiler_nspawn_opts(
    *,
    cmd: Iterable[MehStr],
    build_appliance: Subvol,
    ro_bindmounts: Iterable[Tuple[AnyStr, AnyStr]] = (),
    rw_bindmounts: Iterable[Tuple[AnyStr, AnyStr]] = (),
):
    """
    Returns the minimum necessary options to have a suitable BA nspawn that
    supports layer compilation.
    """
    return new_nspawn_opts(
        cmd=cmd,
        # Buck2 $() macros give us repo-relative paths.
        chdir=repo_config().repo_root,
        # Needed to btrfs receive subvol sendstreams
        allow_mknod=True,
        layer=build_appliance,
        user=pwd.getpwnam("root"),
        bind_repo_ro=True,
        bind_artifacts_dir_rw=True,
        hostname=hostname_for_compiler_in_ba(),
        bindmount_ro=ro_bindmounts,
        bindmount_rw=rw_bindmounts,
    )


def compile_items_to_subvol(
    *,
    subvol: Subvol,
    layer_opts: LayerOpts,
    iter_items: Iterable[ImageItem],
) -> None:
    """
    IMPORTANT: This function will build many compiler items that assume they
    are running inside a specific BA environment that conforms with the options
    in `get_compiler_nspawn_opts`. If you run this outside that context, you
    are at risk of corrupting your host's filesystem!
    """
    dep_graph = DependencyGraph(
        iter_items=iter_items,
        layer_target=str(layer_opts.layer_target),
    )
    # Creating all the builders up-front lets phases validate their input
    for builder in [
        builder_maker(items, layer_opts)
        for builder_maker, items in dep_graph.ordered_phases()
    ]:
        builder(subvol)

    # We cannot validate or sort `ImageItem`s until the phases are
    # materialized since the items may depend on the output of the phases.
    for par_items in dep_graph.gen_dependency_order_items(
        PhasesProvideItem(from_target=layer_opts.layer_target, subvol=subvol)
    ):
        # Some items exist just for dependency resolution / ordering. Make sure
        # to filter those out before trying to do `item.build`
        par_items = [item for item in par_items if hasattr(item, "build")]
        if not par_items:  # pragma: no cover
            continue
        for item in par_items:
            # pyre-fixme[16]: `antlir.compiler.items.common.ImageItem` has
            # no attribute `build`.
            item.build(subvol, layer_opts)


def main() -> None:  # pragma: no cover
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "compile_items_to_subvol_kwargs",
        type=lambda s: pickle.loads(bytes(s, encoding="utf-8")),
    )
    args = parser.parse_args()

    compile_items_to_subvol(**args.compile_items_to_subvol_kwargs)


# This is covered by test-rpm-replay
if __name__ == "__main__":
    main()  # pragma: no cover
