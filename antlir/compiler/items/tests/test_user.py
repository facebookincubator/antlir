#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from antlir.compiler.items.group import GROUP_FILE_PATH
from antlir.compiler.items.tests.common import BaseItemTestCase
from antlir.compiler.items.user import (
    _read_passwd_file,
    _read_shadow_file,
    _write_passwd_file,
    _write_shadow_file,
    new_passwd_file_line,
    new_shadow_file_line,
    PASSWD_FILE_PATH,
    PasswdFile,
    PasswdFileLine,
    pwconv,
    SHADOW_DEFAULT_LASTCHANGED_DAYS,
    SHADOW_FILE_PATH,
    ShadowFile,
    ShadowFileLine,
    UserItem,
)

from antlir.compiler.requires_provides import (
    ProvidesUser,
    RequireFile,
    RequireGroup,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes, with_temp_subvols
from antlir.tests.layer_resource import layer_resource_subvol


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

# Corresponding sample valid /etc/shadow to above passwd
_SAMPLE_ETC_SHADOW = """root:!!:555::::::
bin:!!:555::::::
daemon:!!:555::::::
adm:!!:555::::::
lp:!!:555::::::
sync:!!:555::::::
shutdown:!!:555::::::
halt:!!:555::::::
mail:!!:555::::::
operator:!!:555::::::
games:!!:555::::::
ftp:!!:555::::::
nobody:!!:555::::::
"""


class PwconvTest(unittest.TestCase):
    def test_pwconv(self):
        # Start with a passwd file, and generate a shadow from it
        pf = PasswdFile(_SAMPLE_ETC_PASSWD)
        sf = ShadowFile(pwconv(pf))
        # Verify that number of lines in passwd and shadow is the same
        self.assertEqual(
            len(pf.lines),
            len(sf.lines),
        )
        # Verify that the users are identical and identically ordered
        # in passwd and shadow
        pf_users = []
        sf_users = []
        for entry in pf.nameToUID:
            pf_users.append(entry)
        for entry in sf.lines:
            sf_users.append(entry)
        self.assertEqual(pf_users, sf_users)

    def test_invalid_passwd(self):
        pf = """bin:x:1:1:bin:/bin:/sbin/nologin
bin:x:1:1:bin:/bin:/sbin/nologin
"""
        with self.assertRaisesRegex(
            RuntimeError,
            r"^Duplicate username in shadow file: bin",
        ):
            ShadowFile(pwconv(pf))

    def test_default_lastchanged(self):
        pf = PasswdFile(_SAMPLE_ETC_PASSWD)
        sf = ShadowFile(pwconv(pf))
        self.assertEqual(
            SHADOW_DEFAULT_LASTCHANGED_DAYS, sf.lines["root"].lastchanged
        )


class ShadowFileLineTest(unittest.TestCase):
    def test_new_shadow_file_line(self):
        # pass invalid number of fields
        with self.assertRaises(RuntimeError):
            new_shadow_file_line("foo:bar")
        # verify a valid ShadowFileLine is returned
        self.assertEqual(
            new_shadow_file_line("pesign:!!:18609::::::"),
            ShadowFileLine("pesign", "!!", 18609, -1, -1, -1, -1, -1, ""),
        )


class ShadowFileTest(unittest.TestCase):
    def test_init(self):
        sf = ShadowFile(_SAMPLE_ETC_SHADOW)
        self.assertEqual(
            list(sf.lines.values()),
            [
                new_shadow_file_line(line)
                for line in _SAMPLE_ETC_SHADOW.strip().split("\n")
            ],
        )

    def test_init_invalid_file_line(self):
        for shadow_file in [
            "a\n",
            "a:b:c\n",
            """pesign:!!:18609::::::
NO
pesign3:!!:18609::::::
""",
        ]:

            with self.assertRaisesRegex(
                RuntimeError, r"^Invalid shadow file line "
            ):
                ShadowFile(shadow_file)

    def test_init_duplicate_name(self):
        with self.assertRaisesRegex(
            RuntimeError, r"^Duplicate username in shadow file"
        ):
            ShadowFile(
                """pesign:!!:18609::::::
pesign:!!:1234::::::
""",
            )

    def test_add(self):
        sf = ShadowFile()
        self.assertEqual(list(sf.lines.values()), [])
        sf.add(new_shadow_file_line("pesign:!!:18609::::::"))
        self.assertEqual(
            list(sf.lines.values()),
            [
                ShadowFileLine(
                    name="pesign",
                    encrypted_passwd="!!",
                    lastchanged=18609,
                    min_age=-1,
                    max_age=-1,
                    warning_period=-1,
                    inactivity=-1,
                    expiration=-1,
                    reserved_field="",
                )
            ],
        )

        with self.assertRaisesRegex(
            ValueError,
            r"^new user pesign:!!:1234:::::: conflicts "
            "with pesign:!!:18609::::::$",
        ):
            sf.add(new_shadow_file_line("pesign:!!:1234::::::"))

    def test_str(self):
        self.assertEqual(
            str(ShadowFile(_SAMPLE_ETC_SHADOW)), _SAMPLE_ETC_SHADOW
        )


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

    def test_get_uid(self) -> None:
        pf = PasswdFile()
        pf.add(new_passwd_file_line("root:x:0:0:root:/root:/bin/bash"))
        pf.add(new_passwd_file_line("antlir:x:10:10:antlir:/antlir:/bin/bash"))
        self.assertEqual(pf.uid("root"), 0)
        self.assertEqual(pf.uid("antlir"), 10)
        self.assertIsNone(pf.uid("nope"))


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
                RequireFile(path=Path("/etc/passwd")),
                RequireFile(path=Path("/etc/group")),
                RequireFile(path=Path("/bin/bash")),
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
                RequireFile(path=Path("/etc/passwd")),
                RequireFile(path=Path("/etc/group")),
                RequireFile(path=Path("/sbin/nologin")),
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
        # Verify that shadow file has same number of lines as passwd
        self.assertEqual(
            len(sv.read_path_text(PASSWD_FILE_PATH).split("\n")),
            len(sv.read_path_text(SHADOW_FILE_PATH).split("\n")),
        )

    @with_temp_subvols
    def test_build_with_existing_shadow(self, ts: TempSubvolumes):
        """This test does a user add with an existing shadow file, and
        validates that we keep the existing shadow file entries when
        adding a new user.
        """
        default_crypt_string = "Not_a_real_crypt_string"
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
        sv.overwrite_path_as_root(
            SHADOW_FILE_PATH,
            f"""root:!!:1234::::::
bin:{default_crypt_string}:1234::::::
daemon:!!:1234::::::
adm:!!:1234::::::
lp:!!:1234::::::
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
        # Validate that the existing password hash is unchanged
        sf = ShadowFile(_read_shadow_file(sv))
        self.assertEqual(
            sf.lines["bin"].encrypted_passwd,
            default_crypt_string,
        )

    def test_user_groups_from_scratch(self):
        test_subvol = layer_resource_subvol(
            __package__, "test-layer-users-groups-from-scratch"
        )
        passwd = _read_passwd_file(test_subvol)
        self.assertIn("example", passwd)


class ReadWriteMethodsTest(unittest.TestCase):
    @with_temp_subvols
    def test_write_read_passwd_file(self, ts: TempSubvolumes):
        sv = ts.create("test_rw")
        sv.run_as_root(["mkdir", "-p", sv.path("/etc")]).check_returncode()
        _write_passwd_file(sv, _SAMPLE_ETC_PASSWD)
        self.assertEqual(
            _SAMPLE_ETC_PASSWD,
            _read_passwd_file(sv),
        )
        _write_shadow_file(sv, _SAMPLE_ETC_SHADOW)
        self.assertEqual(
            _SAMPLE_ETC_SHADOW,
            _read_shadow_file(sv),
        )
