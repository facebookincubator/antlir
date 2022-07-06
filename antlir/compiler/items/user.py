#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import collections
import re
from dataclasses import dataclass
from typing import AnyStr, Dict, Generator, NamedTuple, Optional, OrderedDict

from antlir.bzl.image.feature.usergroup import user_t
from antlir.compiler.requires_provides import (
    Provider,
    ProvidesUser,
    RequireFile,
    RequireGroup,
    Requirement,
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
    USERGROUP_LOCK,
)


# Default values from /etc/login.defs
_UID_MIN = 1000
_UID_MAX = 60000


PASSWD_FILE_PATH = Path("/etc/passwd")
SHADOW_FILE_PATH = Path("/etc/shadow")

# When we generate shadow files, the default number of days
# since password last changed
SHADOW_DEFAULT_LASTCHANGED_DAYS = 555


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
    lines: OrderedDict[int, PasswdFileLine]
    nameToUID: Dict[str, int]

    def __init__(self, passwd_file: str = ""):
        """
        Parse `passwd_file` as /etc/passwd file. See `man 5 passwd`
        """
        self.lines = collections.OrderedDict()
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

    def uid(self, name: str) -> Optional[int]:
        for line in self.lines.values():
            if line.name == name:
                return line.uid

        return None


@dataclass
class ShadowFileLine:
    """
    ShadowFileLine represents a single line in a ShadowFile.

    Each line of this file describes a single user, and contains 9 fields,
    separated by colons. Please read carefully the definitions of these fields
    in `man 5 shadow` if you are working with shadow files.

    Currently we create users with default shadow fields and do not support
    setting these in user_t ; that can be changed in the future if we wanted to
    support features such as setting password parameters such as min or max age.

    shadow files contain a bunch of integer fields that are, in practice, almost
    entirely unused. We use the convention of setting them to -1 to
    denote that they should be empty in the generated shadow file (because 0
    usually means something else for the field).
    """

    name: str
    encrypted_passwd: str = "!!"
    lastchanged: int = SHADOW_DEFAULT_LASTCHANGED_DAYS
    min_age: int = -1
    max_age: int = -1
    warning_period: int = -1
    inactivity: int = -1
    expiration: int = -1
    reserved_field: str = ""

    def __str__(self):
        return ":".join(
            (
                self.name,
                self.encrypted_passwd,
                str(self.lastchanged) if self.lastchanged >= 0 else "",
                str(self.min_age) if self.min_age >= 0 else "",
                str(self.max_age) if self.max_age >= 0 else "",
                str(self.warning_period) if self.warning_period >= 0 else "",
                str(self.inactivity) if self.inactivity >= 0 else "",
                str(self.expiration) if self.expiration >= 0 else "",
                self.reserved_field,
            )
        )


def new_shadow_file_line(line: str) -> ShadowFileLine:
    """Turns a line from a shadow file into a ShadowFileLine"""
    expected_fields = 9
    fields = line.split(":")
    if len(fields) != expected_fields:
        raise RuntimeError(
            f"Invalid shadow file line (expected {expected_fields}, "
            f"got {len(fields)}): {line}"
        )
    return ShadowFileLine(
        name=fields[0],
        encrypted_passwd=fields[1],
        lastchanged=int(fields[2]) if fields[2] else -1,
        min_age=int(fields[3]) if fields[3] else -1,
        max_age=int(fields[4]) if fields[4] else -1,
        warning_period=int(fields[5]) if fields[5] else -1,
        inactivity=int(fields[6]) if fields[6] else -1,
        expiration=int(fields[7]) if fields[7] else -1,
        reserved_field=fields[8],
    )


class ShadowFile:
    lines: OrderedDict[str, ShadowFileLine]

    def __init__(self, shadow_file: str = ""):
        """
        Parse `shadow_file` as /etc/shadow file. See `man 5 shadow`
        """
        self.lines = collections.OrderedDict()
        for l in shadow_file.split("\n"):
            l = l.strip()
            if l == "":
                continue
            sfl = new_shadow_file_line(l)
            if sfl.name in self.lines:
                raise RuntimeError(f"Duplicate username in shadow file: {l}")
            self.lines[sfl.name] = new_shadow_file_line(l)

    def add(self, sfl: ShadowFileLine):
        if sfl.name in self.lines:
            line = self.lines[sfl.name]
            raise ValueError(f"new user {sfl} conflicts with {line}")
        self.lines[sfl.name] = sfl

    def __str__(self):
        return "\n".join((str(sfl) for sfl in self.lines.values())) + "\n"


def pwconv(passwd_file: PasswdFile) -> str:
    """Very similar to UNIX `pwconv` utility: converts a PasswdFile into a string
    corresponding to a shadow file. Fails on duplicate usernames.
    """
    shadow_entries = collections.OrderedDict()
    for l in str(passwd_file).split("\n"):
        l = l.strip()
        if not l:
            continue
        entry = new_passwd_file_line(l)
        name = entry.name
        if name in shadow_entries:
            raise RuntimeError(f"Duplicate username in shadow file: {l}")
        shadow_entries[name] = str(ShadowFileLine(name))
    return "\n".join(shadow_entries[name] for name in shadow_entries) + "\n"


_VALID_USERNAME_RE = re.compile(
    r"^[a-z_]([a-z0-9_-]{0,31}|[a-z0-9_-]{0,30}\$)$"
)


# These provide mocking capabilities for testing
def _read_passwd_file(subvol: Subvol) -> str:
    return subvol.read_path_text(PASSWD_FILE_PATH)


def _write_passwd_file(subvol: Subvol, contents: AnyStr):
    subvol.overwrite_path_as_root(PASSWD_FILE_PATH, str(contents))


def _read_shadow_file(subvol: Subvol) -> str:
    return subvol.read_path_text_as_root(SHADOW_FILE_PATH)


def _write_shadow_file(subvol: Subvol, contents: AnyStr):
    subvol.overwrite_path_as_root(SHADOW_FILE_PATH, str(contents))


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

    # pyre-fixme[9]: layer_opts has type `LayerOpts`; used as `None`.
    def build(self, subvol: Subvol, layer_opts: LayerOpts = None):
        with USERGROUP_LOCK:
            group_file = GroupFile(_read_group_file(subvol))

            # this should already be checked by requires/provides
            assert (
                self.primary_group in group_file.nameToGID
            ), f"primary_group `{self.primary_group}` missing from /etc/group"

            for groupname in self.supplementary_groups:
                group_file.join(groupname, self.name)
            # pyre-fixme[6]: Expected `AnyStr` for 2nd param but got
            # `GroupFile`.
            _write_group_file(subvol, group_file)

            passwd_file = PasswdFile(_read_passwd_file(subvol))
            uid = self.id or passwd_file.next_user_id()
            passwd_file.add(
                PasswdFileLine(
                    name=self.name,
                    uid=uid,
                    gid=group_file.nameToGID[self.primary_group],
                    # pyre-fixme[6]: Expected `str` for 4th param but got
                    # `Optional[str]`.
                    comment=self.comment,
                    directory=self.home_dir,
                    shell=self.shell,
                )
            )
            # pyre-fixme[6]: Expected `AnyStr` for 2nd param but got
            # `PasswdFile`.
            _write_passwd_file(subvol, passwd_file)
            # Read in our current shadow file
            # If we don't already have a shadow file, make one from passwd
            if subvol.path(SHADOW_FILE_PATH).exists():
                shadow_file = ShadowFile(_read_shadow_file(subvol))
                shadow_file.add(
                    ShadowFileLine(
                        name=self.name,
                    )
                )
            else:
                shadow_file = pwconv(passwd_file)
            # pyre-fixme[6]: Expected `AnyStr` for 2nd param but got
            #  `Union[ShadowFile, str]`.
            _write_shadow_file(subvol, shadow_file)
