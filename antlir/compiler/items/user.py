#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from collections import OrderedDict
from typing import Dict, Generator, NamedTuple

from antlir.compiler.requires_provides import (
    Provider,
    ProvidesUser,
)
from antlir.fs_utils import Path


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
                self.comment,
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
