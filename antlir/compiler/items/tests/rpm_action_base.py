#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess

from antlir.bzl_const import BZL_CONST

from antlir.compiler.items.rpm_action import RpmAction, RpmActionItem
from antlir.compiler.items.tests.common import (
    DUMMY_LAYER_OPTS,
    render_subvol,
    with_mocked_temp_volume_dir,
)
from antlir.fs_utils import Path, RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.subvol_utils import Subvol, TempSubvolumes
from pydantic import ValidationError


def create_rpm_action_item(
    from_target: str = "t", flavor_to_version_set=None, **kwargs
):
    flavor_to_version_set = flavor_to_version_set or {
        "antlir_test": BZL_CONST.version_set_allow_all_versions
    }
    return RpmActionItem(
        from_target=from_target,
        flavor_to_version_set=flavor_to_version_set,
        **kwargs,
    )


class RpmActionItemTestBase:
    def _opts(self, *, build_appliance=None, **kwargs):
        return DUMMY_LAYER_OPTS._replace(
            **kwargs,
            build_appliance=build_appliance
            or Subvol("test-build-appliance", already_exists=True),
            rpm_installer=self._YUM_DNF,
            rpm_repo_snapshot=RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR
            / self._YUM_DNF.value,
        )

    def _check_rpm_action_item_build_appliance(self, ba_path: Path) -> None:
        self._check_rpm_action_item(
            layer_opts=self._opts(build_appliance=ba_path),
        )

    @with_mocked_temp_volume_dir
    def _check_rpm_action_item(self, layer_opts) -> None:
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("rpm_action")
            # pyre-fixme[16]: `RpmActionItemTestBase` has no attribute `assertEqual`.
            self.assertEqual(["(Dir)", {}], render_subvol(subvol))

            # The empty action is a no-op
            RpmActionItem.get_phase_builder([], layer_opts)(subvol)
            self.assertEqual(["(Dir)", {}], render_subvol(subvol))

            # `yum-dnf-from-snapshot` needs a `/.meta` directory to work
            subvol.run_as_root(["mkdir", subvol.path(".meta")])
            self.assertEqual(
                # No `opts/artifacts_may_require_repo` here because we directly
                # created the subvol instead of using an Item.
                ["(Dir)", {".meta": ["(Dir)", {}]}],
                render_subvol(subvol),
            )

            # Specifying RPM versions is prohibited
            # pyre-fixme[16]: `RpmActionItemTestBase` has no attribute `assertRaises`.
            with self.assertRaises(subprocess.CalledProcessError):
                RpmActionItem.get_phase_builder(
                    [
                        create_rpm_action_item(
                            from_target="m",
                            name="rpm-test-milk-2.71",
                            source=None,
                            action=RpmAction.install,
                        )
                    ],
                    layer_opts,
                )(subvol)

            # Cannot pass both `name` and `source`
            # pyre-fixme[16]: `RpmActionItemTestBase` has no attribute
            #  `assertRaisesRegex`.
            with self.assertRaisesRegex(
                ValidationError,
                "Exactly one of `name` or `source` must be set .*",
            ):
                RpmActionItem.get_phase_builder(
                    [
                        create_rpm_action_item(
                            from_target="m",
                            name="rpm-test-milk",
                            source="rpm-test-milk",
                            action=RpmAction.install,
                        )
                    ],
                    layer_opts,
                )(subvol)

            RpmActionItem.get_phase_builder(
                [
                    create_rpm_action_item(name=n, action=RpmAction.install)
                    for n in [
                        # This specific RPM contains `/bin/sh` and a
                        # post-install script to test `/dev/null` isolation.
                        "rpm-test-milk",
                        "rpm-test-carrot",
                    ]
                ],
                layer_opts,
            )(subvol)
            # Clean up the `dnf`, `yum` & `rpm` litter before checking the
            # packages.  Maybe fixme: We end up not asserting ownership /
            # permissions / etc on directories like /var and /dev.
            subvol.run_as_root(
                [
                    "rm",
                    "-rf",
                    # Annotate all paths since `sudo rm -rf` is scary.
                    subvol.path("var/lib/rpm"),
                    subvol.path("var/lib/yum"),
                    subvol.path("var/lib/dnf"),
                    subvol.path("var/log/yum.log"),
                    *(
                        subvol.path("var/log/" + log)
                        for log in [
                            "yum.log",
                            "dnf.log",
                            "dnf.librepo.log",
                            "dnf.rpm.log",
                            "hawkey.log",
                        ]
                    ),
                    subvol.path("usr/lib/.build-id"),
                    subvol.path("bin/sh"),
                ]
            )
            # pyre-fixme[16]: `RpmActionItemTestBase` has no attribute `_YUM_DNF`.
            if self._YUM_DNF == YumDnf.dnf:
                subvol.run_as_root(
                    [
                        "rmdir",
                        subvol.path("etc/dnf/modules.d"),
                        subvol.path("etc/dnf"),
                        subvol.path("etc"),
                    ]
                )
            subvol.run_as_root(
                [
                    "rmdir",
                    subvol.path("dev"),  # made by yum_dnf_from_snapshot.py
                    subvol.path(".meta"),
                    subvol.path("var/lib"),
                    subvol.path("var/log"),
                    subvol.path("var/tmp"),
                    subvol.path("var"),
                    subvol.path("usr/lib"),
                    subvol.path("bin"),
                ]
            )
            # `/var/cache/{dnf,yum}` should not be populated by
            # `RpmActionItem`.  It is important that the cache of built
            # images be empty to avoid bloating the distributed image size.
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "rpm_test": [
                            "(Dir)",
                            {
                                "carrot.txt": ["(File d13)"],
                                "milk.txt": ["(File d12)"],
                                "post.txt": ["(File d6)"],
                            },
                        ],
                        "usr": ["(Dir)", {}],
                    },
                ],
                render_subvol(subvol),
            )
