#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from antlir.fs_utils import Path

from ..user import PasswdFile, PasswdFileLine, new_passwd_file_line


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
