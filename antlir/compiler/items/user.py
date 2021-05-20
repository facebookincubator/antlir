#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import re
from collections import OrderedDict
from typing import (
    AnyStr,
    Dict,
    Generator,
    NamedTuple,
)

from antlir.compiler.requires_provides import (
    Provider,
    RequireGroup,
    Requirement,
    ProvidesUser,
    RequireDirectory,
    RequireFile,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol
from pydantic import validator

from .common import ImageItem, LayerOpts
from .group import (
    _read_group_file,
    _write_group_file,
    GROUP_FILE_PATH,
    GroupFile,
)
from .user_t import user_t


# Default values from /etc/login.defs
_UID_MIN = 1000
_UID_MAX = 60000


PASSWD_FILE_PATH = Path("/etc/passwd")


class PasswdFileLine(NamedTuple):
    """
    PasswdFileLine represents a single line in a PasswdFile.

    From `man 5 passwd`:

       Each line of the file describes a single user, and contains seven
       colon-separated fields:

           name:password:UID:GID:GECOS:directory:shell

    The password field is omitted on purpose and will always be 'x' since we
    only support shadowed passwords.

    GECOS is aka the comment field.
    """

    name: str
    uid: int
    gid: int
    comment: str
    directory: Path
    shell: Path

    def __str__(self):
        return ":".join(
            (
                self.name,
                "x",
                str(self.uid),
                str(self.gid),
                self.comment or "",
                self.directory.decode(),
                self.shell.decode(),
            )
        )


def new_passwd_file_line(line: str) -> PasswdFileLine:
    fields = line.split(":")
    if len(fields) != 7:
        raise RuntimeError(f"Invalid passwd file line: {line}")
    return PasswdFileLine(
        name=fields[0],
        uid=int(fields[2]),
        gid=int(fields[3]),
        comment=fields[4],
        directory=Path(fields[5]),
        shell=Path(fields[6]),
    )


class PasswdFile:
    lines: "OrderedDict[int, PasswdFileLine]"
    nameToUID: Dict[str, int]

    def __init__(self, passwd_file: str = ""):
        """
        Parse `passwd_file` as /etc/passwd file. See `man 5 passwd`
        """
        self.lines = OrderedDict()
        self.nameToUID = {}
        for l in passwd_file.split("\n"):
            l = l.strip()
            if l == "":
                continue
            pfl = new_passwd_file_line(l)
            if pfl.uid in self.lines:
                raise RuntimeError(f"Duplicate UID in passwd file: {l}")
            self.lines[pfl.uid] = pfl
            if pfl.name in self.nameToUID:
                raise RuntimeError(f"Duplicate username in passwd file: {l}")
            self.nameToUID[pfl.name] = pfl.uid

    def next_user_id(self) -> int:
        # Future: read /etc/login.defs and respect UID_MIN/UID_MAX?
        next_uid = _UID_MIN
        for uid in self.lines:
            if uid > _UID_MAX:
                continue
            if uid >= next_uid:
                next_uid = uid + 1
        if next_uid > _UID_MAX:
            raise RuntimeError(f"user ids exhausted (max: {_UID_MAX})")
        return next_uid

    def add(self, pfl: PasswdFileLine):
        if pfl.uid in self.lines:
            line = self.lines[pfl.uid]
            raise ValueError(f"new user {pfl} conflicts with {line}")
        if pfl.name in self.nameToUID:
            raise ValueError(f"user {pfl.name} already exists")
        self.lines[pfl.uid] = pfl
        self.nameToUID[pfl.name] = pfl.uid

    def provides(self) -> Generator[Provider, None, None]:
        for name in self.nameToUID:
            yield ProvidesUser(name)

    def __str__(self):
        return "\n".join((str(pfl) for pfl in self.lines.values())) + "\n"


_VALID_USERNAME_RE = re.compile(
    r"^[a-z_]([a-z0-9_-]{0,31}|[a-z0-9_-]{0,30}\$)$"
)


# These provide mocking capabilities for testing
def _read_passwd_file(subvol: Subvol) -> str:
    return subvol.read_path_text(PASSWD_FILE_PATH)


def _write_passwd_file(subvol: Subvol, contents: AnyStr):
    subvol.overwrite_path_as_root(PASSWD_FILE_PATH, str(contents))


class UserItem(user_t, ImageItem):
    @validator("name")
    def _validate_name(cls, name):  # noqa B902
        # Validators are classmethods but flake8 doesn't catch that.
        if len(name) < 1 or len(name) > 32:
            raise ValueError(f"username `{name}` must be 1-32 characters")
        if not _VALID_USERNAME_RE.match(name):
            raise ValueError(f"username `{name}` invalid")
        return name

    def requires(self) -> Generator[Requirement, None, None]:
        yield RequireGroup(self.primary_group)
        for groupname in self.supplementary_groups:
            yield RequireGroup(groupname)

        # The root user is *always* available, even without a
        # passwd db
        if self.name != "root":
            yield RequireFile(path=GROUP_FILE_PATH)
            yield RequireFile(path=PASSWD_FILE_PATH)

        yield RequireFile(path=self.shell)

    def provides(self) -> Generator[Provider, None, None]:
        yield ProvidesUser(self.name)

    def build(self, subvol: Subvol, layer_opts: LayerOpts = None):
        group_file = GroupFile(_read_group_file(subvol))

        # this should already be checked by requires/provides
        assert (
            self.primary_group in group_file.nameToGID
        ), f"primary_group `{self.primary_group}` missing from /etc/group"

        for groupname in self.supplementary_groups:
            group_file.join(groupname, self.name)
        _write_group_file(subvol, group_file)

        passwd_file = PasswdFile(_read_passwd_file(subvol))
        uid = self.id or passwd_file.next_user_id()
        passwd_file.add(
            PasswdFileLine(
                name=self.name,
                uid=uid,
                gid=group_file.nameToGID[self.primary_group],
                comment=self.comment,
                directory=self.home_dir,
                shell=self.shell,
            )
        )
        _write_passwd_file(subvol, passwd_file)
