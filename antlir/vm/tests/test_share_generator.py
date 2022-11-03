#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import os
import subprocess
import tempfile
from dataclasses import dataclass
from typing import Optional

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase
from antlir.vm.share import Plan9Export, Share


@dataclass(frozen=True)
class TestShare(object):
    share: Share
    unit: Optional[str]
    contents: Optional[str]


TEST_SHARES = [
    TestShare(
        Plan9Export(path=Path("/tmp/hello"), mountpoint=Path("/tmp/hello")),
        "tmp-hello.mount",
        """[Unit]
Description=Mount fs0 at /tmp/hello
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What=fs0
Where=/tmp/hello
Type=9p
Options=version=9p2000.L,posixacl,cache=loose,ro,msize=209715200
""",
    ),
    TestShare(
        Plan9Export(
            path=Path("/usr/tag"),
            mountpoint=Path("/usr/tag"),
            mount_tag="explicit_tag",
        ),
        "usr-tag.mount",
        """[Unit]
Description=Mount explicit_tag at /usr/tag
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What=explicit_tag
Where=/usr/tag
Type=9p
Options=version=9p2000.L,posixacl,cache=loose,ro,msize=209715200
""",
    ),
    TestShare(
        Plan9Export(
            path=Path("/tmp/not-included"),
            generator=False,
        ),
        None,
        None,
    ),
    TestShare(
        Plan9Export(path=Path("/some/host/path"), mountpoint=Path("/guest/other")),
        "guest-other.mount",
        """[Unit]
Description=Mount fs2 at /guest/other
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What=fs2
Where=/guest/other
Type=9p
Options=version=9p2000.L,posixacl,cache=loose,ro,msize=209715200
""",
    ),
    TestShare(
        Plan9Export(
            path=Path("/tmp/hello_rw"),
            mountpoint=Path("/tmp/hello_rw"),
            readonly=False,
        ),
        "tmp-hello_rw.mount",
        """[Unit]
Description=Mount fs3 at /tmp/hello_rw
Requires=systemd-modules-load.service
After=systemd-modules-load.service
Before=local-fs.target

[Mount]
What=fs3
Where=/tmp/hello_rw
Type=9p
Options=version=9p2000.L,posixacl,cache=none,rw,msize=209715200
""",
    ),
]


class TestShareGenerator(AntlirTestCase):
    def test_units(self):
        with importlib.resources.path(
            __package__, "mount-generator"
        ) as generator, Share.export_spec(
            [s.share for s in TEST_SHARES]
        ) as share, tempfile.TemporaryDirectory() as outdir:
            subprocess.run(
                [generator, outdir], env={"EXPORTS_DIR": share.path}, check=True
            )

            units = {s.unit for s in TEST_SHARES if s.unit}

            self.assertEqual(
                set(os.listdir(outdir)),
                units.union(
                    {"local-fs.target.requires", "workload-pre.target.requires"}
                ),
            )
            self.assertEqual(
                set(os.listdir(os.path.join(outdir, "local-fs.target.requires"))),
                units,
            )
            for share in TEST_SHARES:
                if not share.share.generator:
                    continue
                # check that the mount units have the expected content
                with open(os.path.join(outdir, share.unit)) as f:
                    self.assertEqual(f.read(), share.contents)

                # set as a requirement of local-fs.target
                self.assertEqual(
                    os.readlink(
                        os.path.join(outdir, "local-fs.target.requires", share.unit)
                    ),
                    os.path.join(outdir, share.unit),
                )
