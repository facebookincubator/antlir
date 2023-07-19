#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest
import uuid
from contextlib import contextmanager
from typing import FrozenSet
from unittest import mock

from antlir.config import antlir_dep
from antlir.fs_utils import create_ro, META_DIR, Path, temp_dir

from antlir.rpm import yum_dnf_from_snapshot
from antlir.rpm.find_snapshot import snapshot_install_dir
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.subvol_utils import Subvol
from antlir.tests.subvol_helpers import check_common_rpm_render, pop_path, render_subvol

_INSTALL_ARGS = ["install", "--assumeyes", "rpm-test-carrot", "rpm-test-milk"]
_SNAPSHOT_DIR = snapshot_install_dir(antlir_dep("rpm:repo-snapshot-for-tests"))


def _temp_subvol(name: str):
    return Subvol(Path("/") / f"{name}-{uuid.uuid4().hex}").create().delete_on_exit()


class YumDnfFromSnapshotTestImpl:
    def setUp(self) -> None:  # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute `maxDiff`.
        self.maxDiff = 12345

    def _yum_dnf_from_snapshot(self, **kwargs) -> None:
        yum_dnf_from_snapshot.yum_dnf_from_snapshot(
            # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute `_YUM_DNF`.
            yum_dnf=self._YUM_DNF,
            snapshot_dir=_SNAPSHOT_DIR,
            **kwargs,
        )

    @contextmanager
    def _install(
        self,
        *,
        protected_paths,
        install_args=None,
        # Create IMAGE_ROOT/<META_DIR> by default, since it's always
        # protected, if it exists.
        extra_mkdirs: FrozenSet[str] = frozenset([META_DIR.decode()]),
    ):
        if install_args is None:
            install_args = _INSTALL_ARGS
        with temp_dir() as install_root:
            for p in set(protected_paths) | extra_mkdirs:
                if p.endswith("/"):
                    os.makedirs(install_root / p)
                else:
                    os.makedirs(os.path.dirname(install_root / p))
                    with open(install_root / p, "wb"):
                        pass
            # Note: this can't use `_yum_using_build_appliance` because that
            # would lose coverage info on `yum_dnf_from_snapshot.py`.  On
            # the other hand, running this test against the host is fragile
            # since it depends on the system packages available on CI
            # containers.  For this reason, this entire test is an
            # `image.python_unittest` that runs in a build appliance.
            self._yum_dnf_from_snapshot(
                protected_paths=protected_paths,
                yum_dnf_args=[f"--installroot={install_root}", *install_args],
            )
            yield install_root

    def _check_installed_content(self, install_root, installed_content) -> None:
        # Remove known content so we can check there is nothing else.
        remove = []

        # Check that the RPMs installed their payload.
        for path, content in installed_content.items():
            remove.append(install_root / "rpm_test" / path)
            with open(remove[-1]) as f:
                # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute
                #  `assertEqual`.
                self.assertEqual(content, f.read())

        # Remove /bin/sh
        remove.append(install_root / "bin/sh")

        # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute `_YUM_DNF`.
        prog_name = self._YUM_DNF.value

        # `yum` & `dnf` also write some indexes & metadata.
        for path in [
            f"var/lib/{prog_name}",
            "var/lib/rpm",
            "usr/lib/.build-id",
        ]:
            remove.append(install_root / path)
            # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute
            #  `assertTrue`.
            self.assertTrue(os.path.isdir(remove[-1]), remove[-1])
        remove.append(install_root / "var/log/dnf.log")
        self.assertTrue(os.path.exists(remove[-1]))
        for logfile in ["dnf.librepo.log", "dnf.rpm.log", "hawkey.log"]:
            remove.append(install_root / "var/log" / logfile)

        # Check that the above list of paths is complete.
        for path in remove:
            # We're running rm -rf as `root`, better be careful.
            self.assertTrue(path.startswith(install_root))
            # Most files are owned by root, so the sudo is needed.
            subprocess.run(["sudo", "rm", "-rf", path], check=True)

        subprocess.run(
            [
                "sudo",
                "rmdir",
                "rpm_test",
                "usr/lib",
                "usr",
                "var/lib",
                "var/log",
                "var/tmp",
                "var",
                "bin",
                "etc/dnf/modules.d",
                "etc/dnf",
                "etc",
            ],
            check=True,
            cwd=install_root,
        )
        required_dirs = {b"dev", META_DIR.normpath()}
        self.assertEqual(required_dirs, set(install_root.listdir()))
        for d in required_dirs:
            self.assertEqual([], (install_root / d).listdir())

    def test_verify_contents_of_install_from_snapshot(self) -> None:
        milk = {
            "milk.txt": "milk 2.71 8\n",
            "post.txt": "stuff\n",  # From `milk-2.71` post-install
        }
        with self._install(protected_paths=[META_DIR.decode()]) as install_root:
            self._check_installed_content(
                install_root, {**milk, "carrot.txt": "carrot 2 rc0\n"}
            )

        # Fail when installing a package by its Provides: name, even when there
        # are more than one package in the list. Yum will only exit with an
        # error code here when specific options are explicitly set in the
        # yum.conf file.
        def _install_by_provides():
            return self._install(
                protected_paths=[],
                install_args=[
                    "install-n",
                    "--assumeyes",
                    "virtual-carrot-2",
                    "rpm-test-milk",
                ],
            )

        # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute `_YUM_DNF`.
        # DNF allows `install-n` to install by a "Provides:" name. We don't
        # particularly like the inconsistency with the behavior of yum, but
        # since we have a test for it, let's assert it here.
        with _install_by_provides() as install_root:
            self._check_installed_content(
                install_root, {**milk, "carrot.txt": "carrot 2 rc0\n"}
            )

    def test_fail_to_write_to_protected_path(self) -> None:
        # Nothing fails with no specified protection, or with META_DIR
        # explicitly protected, whether or not META_DIR exists.
        for p in [[], [META_DIR.decode()]]:
            with self._install(protected_paths=p):
                pass
            # pyre-fixme[6]: For 2nd param expected `frozenset[str]` but got
            #  `Set[Variable[_T]]`.
            with self._install(protected_paths=p, extra_mkdirs=set()):
                pass
        # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute `assertRaises`.
        with self.assertRaises(subprocess.CalledProcessError) as ctx:
            with self._install(protected_paths=["rpm_test/"]):
                pass
        with self.assertRaises(subprocess.CalledProcessError) as ctx:
            with self._install(protected_paths=["rpm_test/milk.txt"]):
                pass
        # It was none other than `yum install` that failed.
        # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute `assertEqual`.
        self.assertEqual(_INSTALL_ARGS, ctx.exception.cmd[-len(_INSTALL_ARGS) :])

    def test_verify_install_to_container_root(self) -> None:
        # Hack alert: if we run both `{Dnf,Yum}FromSnapshotTestCase` in one
        # test invocation, the package manager that runs will just say that
        # the package is already install, and succeed.  That's OK.
        self._yum_dnf_from_snapshot(
            protected_paths=[],
            yum_dnf_args=[
                # This is implicit: that also covers the "read the conf" code:
                # '--installroot=/',
                # `yum` fails without this since `/usr` is RO in the host BA.
                "--setopt=usr_w_check=false",
                "install-n",
                "--assumeyes",
                "rpm-test-milk-no-sh",
            ],
        )
        # Since we're running on /, asserting the effect on the complete
        # state of the filesystem would only be reasonable if we (a) took a
        # snapshot of the container "before", (b) took a snapshot of the
        # container "after", (c) rendered the incremental sendstream.  Since
        # incremental rendering is not implemented, settle for this basic
        # smoke-test for now.
        # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute `assertEqual`.
        self.assertEqual("lala\n", Path("/rpm_test/milk-no-sh.txt").read_text())
        # Check that our post-install scriptlet worked
        self.assertEqual("stuff\n", Path("/rpm_test/post.txt").read_text())

    @contextmanager
    def _set_up_shadow(self, replacement, to_shadow):
        # Create the mountpoint at the shadowed location, and the file
        # that will shadow it.
        with create_ro(to_shadow, "w"):
            pass
        with create_ro(replacement, "w") as outfile:
            outfile.write("shadows carrot")

        # Shadow the file that `yum` / `dnf` wants to write -- writing to
        # this location will now fail since it's read-only.
        subprocess.check_call(["mount", "-o", "bind,ro", replacement, to_shadow])
        try:
            yield
        finally:
            # Required so that our temporary dirs can be cleaned up.
            subprocess.check_call(["umount", to_shadow])

    def test_update_shadowed(self) -> None:
        with temp_dir() as root, mock.patch.object(
            # Note that the shadowed root is under the install root, since
            # the `rename` runs under chroot.
            yum_dnf_from_snapshot,
            "_LIBRENAME_SHADOWED_PATHS_ROOT",
            Path("/shadow"),
        ):
            os.mkdir(root / META_DIR)
            os.mkdir(root / "rpm_test")
            os.makedirs(root / "shadow/rpm_test")

            to_shadow = root / "rpm_test/carrot.txt"
            replacement = root / "rpm_test/shadows_carrot.txt"
            shadowed = root / "shadow/rpm_test/carrot.txt"

            # Our shadowing setup is supposed to have moved the original here.
            with create_ro(shadowed, "w") as outfile:
                outfile.write("`rpm` writes here")

            with self._set_up_shadow(replacement, to_shadow):
                # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute
                #  `assertEqual`.
                self.assertEqual("shadows carrot", to_shadow.read_text())
                self.assertEqual("`rpm` writes here", shadowed.read_text())

                self._yum_dnf_from_snapshot(
                    protected_paths=[],
                    yum_dnf_args=[
                        f"--installroot={root}",
                        "install",
                        "--assumeyes",
                        "rpm-test-carrot",
                    ],
                )

                # The shadow is still in place
                self.assertEqual("shadows carrot", to_shadow.read_text())
                # But we updated the shadowed file
                self.assertEqual("carrot 2 rc0\n", shadowed.read_text())

    def _check_test_macro_contents(self, install_root: Path, prog) -> None:
        # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute
        # `assertEqual`.
        self.assertEqual(
            {
                YumDnf.dnf: "does not function\n",
                YumDnf.yum: "young urban male?\n",
                # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute
                # `_YUM_DNF`.
            }[self._YUM_DNF],
            Path(install_root / f"etc/rpm/macros.test-{prog}").read_text(),
        )

    # This test shows that when we're installing to /, that our normal "host
    # FS protected paths" do not apply.
    #
    # The `yum` and `dnf` variants of this tests install separate,
    # independent RPMs, so they won't collide even if they run in the same
    # test container.
    def test_install_to_host_etc(self) -> None:
        # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute `_YUM_DNF`.
        prog = self._YUM_DNF.value
        self._yum_dnf_from_snapshot(
            protected_paths=[],
            yum_dnf_args=[
                "install",
                "--assumeyes",
                f"rpm-test-etc-{prog}-macro",
            ],
        )
        self._check_test_macro_contents(Path("/"), prog)

    def test_install_to_installroot_etc(self) -> None:
        with _temp_subvol("test_install_to_installroot_etc") as sv:
            # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute `_YUM_DNF`.
            prog = self._YUM_DNF.value
            self._yum_dnf_from_snapshot(
                protected_paths=[],
                yum_dnf_args=[
                    "install",
                    "--assumeyes",
                    f"--installroot={sv.path()}",
                    f"rpm-test-etc-{prog}-macro",
                ],
            )
            self._check_test_macro_contents(sv.path(), prog)
            r = render_subvol(sv)
            # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute
            #  `assertEqual`.
            self.assertEqual(
                ["(Dir)", {f"macros.test-{prog}": ["(File d18)"]}],
                pop_path(r, "etc/rpm"),
            )
            check_common_rpm_render(self, r, no_meta=True, subvol=sv)

    def test_makecache(self) -> None:
        # The preceding tests implicitly assert that we leak no cache in
        # normal usage.  But `makecache` must write one!  Note that this is
        # not exercised in the expected `--installroot=/` because that would
        # couple the test to the state of the caches in the BA (which should
        # normally be "populated").
        with _temp_subvol("test_makecache") as sv:
            self._yum_dnf_from_snapshot(
                protected_paths=[],
                yum_dnf_args=[
                    "makecache",  # our implementation needs this to be argv[1]
                    f"--installroot={sv.path()}",
                    # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute
                    #  `_YUM_DNF`.
                    *(["fast"] if self._YUM_DNF == "yum" else []),
                ],
            )
            prog = self._YUM_DNF.value
            r = render_subvol(sv)
            antlir_r = pop_path(r, "__antlir__")
            snap_r = pop_path(antlir_r, "rpm/repo-snapshot")
            # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute
            #  `assertEqual`.
            self.assertEqual(["(Dir)", {"rpm": ["(Dir)", {}]}], antlir_r)
            (snap_name,) = snap_r[1].keys()
            cache_ino, cache_contents = pop_path(
                snap_r, f"{snap_name}/{prog}/var/cache/{prog}"
            )
            self.assertEqual("(Dir)", cache_ino)
            # pyre-fixme[16]: `YumDnfFromSnapshotTestImpl` has no attribute
            #  `assertLess`.
            self.assertLess(0, len(cache_contents))
            self.assertEqual(
                ["(Dir)", {"var": ["(Dir)", {"cache": ["(Dir)", {}]}]}],
                pop_path(snap_r, f"{snap_name}/{prog}"),
            )
            self.assertEqual(["(Dir)", {snap_name: ["(Dir)", {}]}], snap_r)
            check_common_rpm_render(self, r, no_meta=True, is_makecache=True, subvol=sv)

    def test_error_repr(self):
        self.assertEqual(
            "YumDnfError(returncode=37)",
            repr(yum_dnf_from_snapshot._YumDnfError(returncode=37, cmd=["unused"])),
        )


class DnfFromSnapshotTestCase(YumDnfFromSnapshotTestImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.dnf
