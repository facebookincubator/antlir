#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from antlir.compiler.requires_provides import (
    ProvidesUser,
    RequireGroup,
    require_directory,
    require_file,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes, with_temp_subvols

from ..group import GROUP_FILE_PATH
from ..user import (
    PASSWD_FILE_PATH,
    PasswdFile,
    PasswdFileLine,
    UserItem,
    new_passwd_file_line,
)
from .common import BaseItemTestCase


class PasswdFileLineTest(unittest.TestCase):
    def test_new_passwd_file_line(self):
        for line in ["", "a", "1:2:3:4:5:6:7:8"]:
            with self.assertRaisesRegex(
                RuntimeError,
                r"^Invalid passwd file line: " + line + r"$",
            ):
                new_passwd_file_line(line)

        self.assertEqual(
            new_passwd_file_line("root:x:0:0:root:/root:/bin/bash"),
            PasswdFileLine(
                name="root",
                uid=0,
                gid=0,
                comment="root",
                directory=Path("/root"),
                shell=Path("/bin/bash"),
            ),
        )

    def test_str(self):
        for line in [
            "root:x:0:0:root:/root:/bin/bash",
            "bin:x:1:1:bin:/bin:/sbin/nologin",
            "daemon:x:2:2:daemon:/sbin:/sbin/nologin",
            "adm:x:3:4:adm:/var/adm:/sbin/nologin",
            "lp:x:4:7:lp:/var/spool/lpd:/sbin/nologin",
            "sync:x:5:0:sync:/sbin:/bin/sync",
            "shutdown:x:6:0:shutdown:/sbin:/sbin/shutdown",
            "halt:x:7:0:halt:/sbin:/sbin/halt",
        ]:
            pfl = new_passwd_file_line(line)
            self.assertEqual(str(pfl), line)


_SAMPLE_ETC_PASSWD = """root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
daemon:x:2:2:daemon:/sbin:/sbin/nologin
adm:x:3:4:adm:/var/adm:/sbin/nologin
lp:x:4:7:lp:/var/spool/lpd:/sbin/nologin
sync:x:5:0:sync:/sbin:/bin/sync
shutdown:x:6:0:shutdown:/sbin:/sbin/shutdown
halt:x:7:0:halt:/sbin:/sbin/halt
mail:x:8:12:mail:/var/spool/mail:/sbin/nologin
operator:x:11:0:operator:/root:/sbin/nologin
games:x:12:100:games:/usr/games:/sbin/nologin
ftp:x:14:50:FTP User:/var/ftp:/sbin/nologin
nobody:x:99:99:Kernel Overflow User:/:/sbin/nologin
"""


class PasswdFileTest(unittest.TestCase):
    def test_init(self):
        pf = PasswdFile(_SAMPLE_ETC_PASSWD)
        self.assertEqual(
            list(pf.lines.values()),
            [
                new_passwd_file_line(line)
                for line in _SAMPLE_ETC_PASSWD.strip().split("\n")
            ],
        )

    def test_init_invalid_file_line(self):
        for passwd_file in [
            "a\n",
            "a:b:c\n",
            """root:x:0:0:root:/root:/bin/bash
foo
bin:x:1:1:bin:/bin:/sbin/nologin
""",
        ]:

            with self.assertRaisesRegex(
                RuntimeError, r"^Invalid passwd file line: "
            ):
                PasswdFile(passwd_file)

    def test_init_duplicate_uid(self):
        with self.assertRaisesRegex(
            RuntimeError, r"^Duplicate UID in passwd file"
        ):
            PasswdFile(
                """root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
bin:x:1:1:bin:/bin:/sbin/nologin
"""
            )

    def test_init_duplicate_username(self):
        with self.assertRaisesRegex(
            RuntimeError, r"^Duplicate username in passwd file"
        ):
            PasswdFile(
                """root:x:0:0:root:/root:/bin/bash
root:x:1:1:bin:/bin:/sbin/nologin
"""
            )

    def test_next_user_id(self):
        pf = PasswdFile("")
        self.assertEqual(pf.next_user_id(), 1000)
        pf = PasswdFile("root:x:0:0:root:/root:/bin/bash\n")
        self.assertEqual(pf.next_user_id(), 1000)
        pf = PasswdFile("myuser:x:1000:100:a user:/home/myuser:/bin/bash\n")
        self.assertEqual(pf.next_user_id(), 1001)
        pf = PasswdFile(
            "myuser:x:1234:100:a user:/home/myuser:/bin/bash\n"
            "myuser2:x:60001:100:another user:/home/myuser2:/bin/bash\n"
        )
        self.assertEqual(pf.next_user_id(), 1235)

    def test_next_user_id_exhausted(self):
        with self.assertRaisesRegex(
            RuntimeError, r"^user ids exhausted \(max: 60000\)$"
        ):
            pf = PasswdFile(
                "myuser:x:60000:100:a user:/home/myuser:/bin/bash\n"
            )
            pf.next_user_id()

    def test_add(self):
        pf = PasswdFile()
        self.assertEqual(list(pf.lines.values()), [])
        pf.add(new_passwd_file_line("root:x:0:0:root:/root:/bin/bash"))
        self.assertEqual(
            list(pf.lines.values()),
            [
                PasswdFileLine(
                    name="root",
                    uid=0,
                    gid=0,
                    comment="root",
                    directory=Path("/root"),
                    shell=Path("/bin/bash"),
                )
            ],
        )
        with self.assertRaisesRegex(
            ValueError,
            r"^new user "
            r"myuser:x:0:0:a user:/home/myuser:/bin/bash "
            r"conflicts with "
            r"root:x:0:0:root:/root:/bin/bash$",
        ):
            pf.add(
                new_passwd_file_line(
                    "myuser:x:0:0:a user:/home/myuser:/bin/bash"
                )
            )
        with self.assertRaisesRegex(ValueError, r"^user root already exists$"):
            pf.add(
                new_passwd_file_line("root:x:1:1:another root:/root:/bin/bash")
            )

    def test_str(self):
        self.assertEqual(
            str(PasswdFile(_SAMPLE_ETC_PASSWD)), _SAMPLE_ETC_PASSWD
        )

    def test_provides(self):
        pf = PasswdFile(_SAMPLE_ETC_PASSWD)
        self.assertEqual(
            set(pf.provides()),
            {
                ProvidesUser("root"),
                ProvidesUser("bin"),
                ProvidesUser("daemon"),
                ProvidesUser("adm"),
                ProvidesUser("lp"),
                ProvidesUser("sync"),
                ProvidesUser("shutdown"),
                ProvidesUser("halt"),
                ProvidesUser("mail"),
                ProvidesUser("operator"),
                ProvidesUser("games"),
                ProvidesUser("ftp"),
                ProvidesUser("nobody"),
            },
        )


class UserItemTest(BaseItemTestCase):
    def test_user(self):
        self._check_item(
            UserItem(
                from_target="t",
                name="newuser",
                primary_group="newuser",
                supplementary_groups=[],
                shell="/bin/bash",
                home_dir="/home/newuser",
            ),
            {ProvidesUser("newuser")},
            {
                RequireGroup("newuser"),
                require_directory(Path("/home/newuser")),
                require_file(Path("/etc/group")),
                require_file(Path("/etc/passwd")),
            },
        )

        self._check_item(
            UserItem(
                from_target="t",
                name="foo",
                primary_group="bar",
                supplementary_groups=["a", "b", "c"],
                shell="/sbin/nologin",
                home_dir="/",
            ),
            {ProvidesUser("foo")},
            {
                RequireGroup("a"),
                RequireGroup("b"),
                RequireGroup("c"),
                RequireGroup("bar"),
                require_directory(Path("/")),
                require_file(Path("/etc/group")),
                require_file(Path("/etc/passwd")),
            },
        )

    def test_validate_name(self):
        for valid_name in [
            "root",
            "fbuser",
            "user123",
            "foo$",
            "_a1b2c3",
        ]:
            self.assertEqual(UserItem._validate_name(valid_name), valid_name)

        for bad_len in [
            "",
            "123456789012345678901234567890123",
        ]:
            with self.assertRaisesRegex(
                ValueError, r"username `.*` must be 1-32 characters"
            ):
                UserItem._validate_name(bad_len)

        for bad in [
            "A",
            "ABC",
            "foo$bar",
            "1abc",
        ]:
            with self.assertRaisesRegex(ValueError, r"username `.*` invalid"):
                UserItem._validate_name(bad)

    @with_temp_subvols
    def test_build(self, ts: TempSubvolumes):
        sv = ts.create("test_build")
        sv.run_as_root(["mkdir", "-p", sv.path("/etc")]).check_returncode()
        sv.overwrite_path_as_root(
            GROUP_FILE_PATH,
            """root:x:0:
bin:x:1:root,daemon
daemon:x:2:root,bin
sys:x:3:root,bin,adm
adm:x:4:
""",
        )
        sv.overwrite_path_as_root(
            PASSWD_FILE_PATH,
            """root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
daemon:x:2:2:daemon:/sbin:/sbin/nologin
adm:x:3:4:adm:/var/adm:/sbin/nologin
lp:x:4:7:lp:/var/spool/lpd:/sbin/nologin
""",
        )
        UserItem(
            from_target="t",
            name="new_user",
            primary_group="sys",
            supplementary_groups=["adm"],
            comment="a new user",
            home_dir="/home/new_user",
            shell="/bin/bash",
        ).build(sv)

        # NB: new_user should only be added to supplementary groups
        self.assertEqual(
            sv.read_path_text(GROUP_FILE_PATH),
            """root:x:0:
bin:x:1:root,daemon
daemon:x:2:root,bin
sys:x:3:root,bin,adm
adm:x:4:new_user
""",
        )
        # NB: new_user's login group ID should be its primary group
        self.assertEqual(
            sv.read_path_text(PASSWD_FILE_PATH),
            """root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
daemon:x:2:2:daemon:/sbin:/sbin/nologin
adm:x:3:4:adm:/var/adm:/sbin/nologin
lp:x:4:7:lp:/var/spool/lpd:/sbin/nologin
new_user:x:1000:3:a new user:/home/new_user:/bin/bash
""",
        )
