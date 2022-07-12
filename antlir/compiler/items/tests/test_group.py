#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys
import unittest

from antlir.compiler.items.group import (
    GROUP_FILE_PATH,
    GroupFile,
    GroupFileLine,
    GroupItem,
)
from antlir.compiler.items.tests.common import BaseItemTestCase

from antlir.compiler.requires_provides import (
    Provider,
    ProvidesGroup,
    RequireFile,
    RequireGroup,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes


_SAMPLE_ETC_GROUP = """root:x:0:
bin:x:1:
daemon:x:2:
sys:x:3:
adm:x:4:
tty:x:5:
disk:x:6:
lp:x:7:
mem:x:8:
kmem:x:9:
wheel:x:10:
cdrom:x:11:
mail:x:12:
man:x:15:
dialout:x:18:
floppy:x:19:
games:x:20:
tape:x:33:
video:x:39:
ftp:x:50:
lock:x:54:
audio:x:63:
users:x:100:
nobody:x:65534:
dbus:x:81:
utmp:x:22:
utempter:x:35:
input:x:999:
kvm:x:36:
render:x:998:
systemd-journal:x:190:
systemd-coredump:x:997:
systemd-network:x:192:
systemd-resolve:x:193:
systemd-timesync:x:996:
tss:x:59:
unbound:x:995:
sshd:x:74:
"""


def augment_group_file(contents: str, groupname: str, gid: int) -> str:
    return contents.strip() + "\n" + groupname + ":x:" + str(gid) + ":\n"


class GroupItemTest(BaseItemTestCase):
    "Tests GroupItem"

    def test_group_item(self) -> None:
        self._check_item(
            GroupItem(from_target="t", name="foo"),
            {ProvidesGroup("foo")},
            {RequireFile(path=Path("/etc/group"))},
        )

    def test_build(self) -> None:
        with TempSubvolumes(Path(sys.argv[0])) as ts:
            sv = ts.create("root")
            sv.run_as_root(["mkdir", sv.path("/etc")]).check_returncode()
            sv.overwrite_path_as_root(GROUP_FILE_PATH, _SAMPLE_ETC_GROUP)
            GroupItem(from_target="t", name="foo").build(sv)
            self.assertEqual(
                augment_group_file(_SAMPLE_ETC_GROUP, "foo", 1000),
                sv.path("/etc/group").read_text(),
            )

    def test_build_twice(self) -> None:
        with TempSubvolumes(Path(sys.argv[0])) as ts:
            sv = ts.create("root")
            sv.run_as_root(["mkdir", sv.path("/etc")]).check_returncode()
            sv.overwrite_path_as_root(GROUP_FILE_PATH, _SAMPLE_ETC_GROUP)
            GroupItem(from_target="t", name="foo").build(sv)
            GroupItem(from_target="t", name="bar").build(sv)
            self.assertEqual(
                augment_group_file(
                    augment_group_file(_SAMPLE_ETC_GROUP, "foo", 1000),
                    "bar",
                    1001,
                ),
                sv.path("/etc/group").read_text(),
            )

    def test_build_with_gid(self) -> None:
        with TempSubvolumes(Path(sys.argv[0])) as ts:
            sv = ts.create("root")
            sv.run_as_root(["mkdir", sv.path("/etc")]).check_returncode()
            sv.overwrite_path_as_root(GROUP_FILE_PATH, _SAMPLE_ETC_GROUP)
            GroupItem(from_target="t", name="foo", id=2000).build(sv)
            self.assertEqual(
                augment_group_file(_SAMPLE_ETC_GROUP, "foo", 2000),
                sv.path("/etc/group").read_text(),
            )


class GroupFileTest(unittest.TestCase):
    def test_init(self) -> None:
        gf = GroupFile("root:x:0:td-agent\nbin:x:1:a,b\n\ndaemon:x:2:\n\n")
        self.assertEqual(
            [
                GroupFileLine(name="root", id=0, members=["td-agent"]),
                GroupFileLine(name="bin", id=1, members=["a", "b"]),
                GroupFileLine(name="daemon", id=2, members=[]),
            ],
            list(gf.lines.values()),
        )

    def test_init_with_bad_line(self) -> None:
        with self.assertRaisesRegex(
            RuntimeError, r"^Invalid line in group file"
        ):
            GroupFile("root:0\n")

    def test_init_with_duplicate_gid(self) -> None:
        with self.assertRaisesRegex(
            RuntimeError, r"^Duplicate GID in group file"
        ):
            GroupFile("root:x:42:\nbin:x:42:")

    def test_init_with_duplicate_groupname(self) -> None:
        with self.assertRaisesRegex(
            RuntimeError, r"^Duplicate groupname in group file"
        ):
            GroupFile("root:x:1:\nroot:x:2:")

    def test_add(self) -> None:
        gf = GroupFile()
        gf.add("group1", 1)
        self.assertEqual(
            [GroupFileLine(name="group1", id=1, members=[])],
            list(gf.lines.values()),
        )
        gf.add("group2", 2)
        gf.add("group3", 3)
        self.assertEqual(
            [
                GroupFileLine(name="group1", id=1, members=[]),
                GroupFileLine(name="group2", id=2, members=[]),
                GroupFileLine(name="group3", id=3, members=[]),
            ],
            list(gf.lines.values()),
        )
        with self.assertRaises(ValueError):
            gf.add("anothergroup2", 2)

    def test_next_group_id(self) -> None:
        gf = GroupFile()
        gf.add("a", 1)
        self.assertEqual(1000, gf.next_group_id())
        gf.add("b", 999)
        self.assertEqual(1000, gf.next_group_id())
        gf.add("c", 1000)
        self.assertEqual(1001, gf.next_group_id())
        gf.add("d", 30000)
        self.assertEqual(30001, gf.next_group_id())
        gf.add("e", 65534)
        self.assertEqual(30001, gf.next_group_id())

    def test_join(self) -> None:
        gf = GroupFile()
        with self.assertRaisesRegex(ValueError, r"^a not found"):
            gf.join("a", "me")
        gf.add("a", 1)
        self.assertEqual(gf.lines[1], GroupFileLine(name="a", id=1, members=[]))
        gf.join("a", "me")
        self.assertEqual(
            gf.lines[1], GroupFileLine(name="a", id=1, members=["me"])
        )
        gf.join("a", "you")
        self.assertEqual(
            gf.lines[1], GroupFileLine(name="a", id=1, members=["me", "you"])
        )

    def test_str(self) -> None:
        gf = GroupFile()
        gf.add("a", 1)
        gf.add("b", 1000)
        gf.join("b", "me")
        gf.join("b", "you")
        gf.add("c", 10000)
        gf.join("c", "me")
        self.assertEqual("a:x:1:\nb:x:1000:me,you\nc:x:10000:me\n", str(gf))

    def test_add_duplicate_name(self) -> None:
        gf = GroupFile()
        gf.add("a", 1)
        with self.assertRaisesRegex(ValueError, r"^group a already exists"):
            gf.add("a", 2)

    def test_provides(self) -> None:
        gf = GroupFile("root:x:0:td-agent\nbin:x:1:a,b\n\ndaemon:x:2:\n\n")
        self.assertEqual(
            {
                ProvidesGroup("root"),
                ProvidesGroup("bin"),
                ProvidesGroup("daemon"),
            },
            set(gf.provides()),
        )

    def test_get_gid(self) -> None:
        gf = GroupFile()
        gf.add("root", 0)
        gf.add("a", 1)
        gf.add("b", 2)
        self.assertEqual(gf.gid("root"), 0)
        self.assertEqual(gf.gid("a"), 1)
        self.assertIsNone(gf.gid("nope"))
