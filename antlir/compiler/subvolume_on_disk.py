#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See the SubvolumeOnDisk docblock."
import json
import logging
import socket
from collections import namedtuple
from typing import Optional
from uuid import UUID

from antlir.btrfsutil import subvolume_info, SubvolumeInfo
from antlir.fs_utils import Path


log = logging.Logger(__name__)

# These constants can represent both JSON keys for
# serialization/deserialization, and namedtuple keys.  Legend:
#  (1) Field in the namedtuple SubvolumeOnDisk
#  (2) Written into the on-disk dictionary format
#  (3) Read from the on-disk dictionary format
_BTRFS_UUID = "btrfs_uuid"  # (1-3)
_BTRFS_PARENT_UUID = "btrfs_parent_uuid"  # (1)
_HOSTNAME = "hostname"  # (1-3)
_SUBVOLUMES_BASE_DIR = "subvolumes_base_dir"  # (1)
_SUBVOLUME_REL_PATH = "subvolume_rel_path"  # (1-3)
_DANGER = "DANGER"  # (2)
_BUILD_APPLIANCE_PATH = "build_appliance_path"  # (1-3)


def _parent_uuid(info: SubvolumeInfo) -> Optional[str]:
    return (
        str(UUID(bytes=info.parent_uuid))
        if info.parent_uuid != b"\0" * 16
        else None
    )


class SubvolumeOnDisk(
    namedtuple(
        "SubvolumeOnDisk",
        [
            _BTRFS_UUID,
            _BTRFS_PARENT_UUID,
            _HOSTNAME,
            _SUBVOLUMES_BASE_DIR,
            _SUBVOLUME_REL_PATH,
            _BUILD_APPLIANCE_PATH,
        ],
        # This assigns default values to the namedtuple starting from
        # the last most argument. This is because it turns arguments
        # in args into kwarg arguments.
        # pyre-fixme[31]: Expression `(None)` is not a valid type.
        defaults=(None,),
    )
):
    """
    This class stores a disk path to a btrfs subvolume (built image layer),
    together with some minimal metadata about the layer.  It knows how to
    serialize & deserialize this metadata to a JSON format that can be
    safely used as as Buck output representing the subvolume.
    """

    def subvolume_path(self) -> Path:
        # pyre-fixme[16]: `SubvolumeOnDisk` has no attribute
        # `subvolumes_base_dir`.
        # pyre-fixme[16]: `SubvolumeOnDisk` has no attribute
        # `subvolume_rel_path`.
        return self.subvolumes_base_dir / self.subvolume_rel_path

    @classmethod
    def from_subvolume_path(
        cls,
        subvol_path: Path,
        subvolumes_dir: Path,
        build_appliance_path: Optional[Path] = None,
    ) -> "SubvolumeOnDisk":
        subvol_rel_path = subvol_path.relpath(subvolumes_dir)
        pieces = subvol_rel_path.split(b"/")
        if pieces[:1] == [b""] or b".." in pieces:
            raise RuntimeError(
                f"{subvol_path} must be located inside the subvolumes "
                f"directory {subvolumes_dir}"
            )
        # This function deliberately does no validation on the fields it
        # populates -- that is done only in `from_serializable_dict`.  We
        # will not commit a buggy structure to disk since
        # `to_serializable_dict` checks the idepmpotency of our
        # serialization-deserialization.
        info = subvolume_info(subvol_path)
        self = cls(
            **{
                _BTRFS_UUID: str(UUID(bytes=info.uuid)),
                _BTRFS_PARENT_UUID: _parent_uuid(info),
                _HOSTNAME: socket.gethostname(),
                _SUBVOLUMES_BASE_DIR: subvolumes_dir,
                _SUBVOLUME_REL_PATH: subvol_rel_path,
                _BUILD_APPLIANCE_PATH: build_appliance_path,
            }
        )
        return self

    @classmethod
    def from_serializable_dict(
        cls,
        # pyre-fixme[2]: Parameter must be annotated.
        d,
        subvolumes_dir: Path,
    ) -> "SubvolumeOnDisk":
        subvol_rel_path = Path(d[_SUBVOLUME_REL_PATH])
        # This is copypasta of subvolume_path() but I need it before
        # creating the object. The assert below keeps them in sync.
        subvol_path = subvolumes_dir / subvol_rel_path
        # This incidentally checks that the subvolume exists and is btrfs.
        info = subvolume_info(subvol_path)
        parent_uuid = _parent_uuid(info)
        self = cls(
            **{
                _BTRFS_UUID: d[_BTRFS_UUID],
                _BTRFS_PARENT_UUID: parent_uuid,
                _HOSTNAME: d[_HOSTNAME],
                _SUBVOLUMES_BASE_DIR: subvolumes_dir,
                _SUBVOLUME_REL_PATH: subvol_rel_path,
                _BUILD_APPLIANCE_PATH: Path(d[_BUILD_APPLIANCE_PATH])
                if _BUILD_APPLIANCE_PATH in d
                else None,
            }
        )
        assert subvol_path == self.subvolume_path(), (d, subvolumes_dir)

        # Check that the relative path is garbage-collectable.
        inner_dir = subvol_rel_path.basename()
        outer_dir = subvol_rel_path.dirname().basename()
        if b":" not in outer_dir or (subvol_rel_path != outer_dir / inner_dir):
            raise RuntimeError(
                "Subvolume must have the form <rule name>:<version>/<subvol>,"
                f" not {subvol_rel_path}"
            )
        outer_dir_content = (subvolumes_dir / outer_dir).listdir()
        # For GC, the wrapper must contain the subvolume, and nothing else.
        if outer_dir_content != [inner_dir]:
            raise RuntimeError(
                f"Subvolume wrapper {outer_dir} contained {outer_dir_content} "
                f"instead of {[inner_dir]}"
            )
        # pyre-fixme[16]: `SubvolumeOnDisk` has no attribute `btrfs_uuid`.
        if UUID(bytes=info.uuid) != UUID(self.btrfs_uuid):
            raise RuntimeError(
                f"UUID in subvolume JSON {self} does not match that of the "
                f"actual subvolume {UUID(bytes=info.uuid)}"
            )
        return self

    # pyre-fixme[3]: Return type must be annotated.
    def to_serializable_dict(self):
        # `subvolumes_dir` is an absolute path to a known location inside
        # the repo.  We must not serialize it inside a Buck outputs, since
        # that will break if the repo is moved.  Instead, we always
        # recompute the path relative to the current subvolumes directory.
        d = {
            # pyre-fixme[16]: `SubvolumeOnDisk` has no attribute `btrfs_uuid`.
            _BTRFS_UUID: self.btrfs_uuid,
            # Not serializing _BTRFS_PARENT_UUID since it's always deduced.
            # pyre-fixme[16]: `SubvolumeOnDisk` has no attribute `hostname`.
            _HOSTNAME: self.hostname,
            # pyre-fixme[16]: `SubvolumeOnDisk` has no attribute `subvolume_rel_path`.
            _SUBVOLUME_REL_PATH: self.subvolume_rel_path.decode(),
            _DANGER: "Do NOT edit manually: this can break future builds, or "
            "break refcounting, causing us to leak or prematurely destroy "
            "subvolumes.",
            **(
                # pyre-fixme[16]: `SubvolumeOnDisk` has no attribute
                #  `build_appliance_path`.
                {_BUILD_APPLIANCE_PATH: self.build_appliance_path.decode()}
                if self.build_appliance_path
                else {}
            ),
        }
        # Self-test -- there should be no way for this assertion to fail
        # pyre-fixme[16]: `SubvolumeOnDisk` has no attribute `subvolumes_base_dir`.
        new_self = self.from_serializable_dict(d, self.subvolumes_base_dir)
        assert (
            self == new_self
        ), f"Got {new_self} from {d}, when serializing {self}"
        return d

    @classmethod
    # pyre-fixme[2]: Parameter must be annotated.
    def from_json_file(cls, infile, subvolumes_dir: Path) -> "SubvolumeOnDisk":
        parsed_json = "<NO JSON PARSED>"
        try:
            parsed_json = json.load(infile)
            return cls.from_serializable_dict(parsed_json, subvolumes_dir)
        except json.JSONDecodeError as ex:
            raise RuntimeError(
                f"Parsing subvolume JSON from {infile}: {ex.doc}"
            ) from ex
        except Exception as ex:
            raise RuntimeError(
                f"Parsed subvolume JSON from {infile}: {parsed_json}"
            ) from ex

    # pyre-fixme[2]: Parameter must be annotated.
    def to_json_file(self, outfile) -> None:
        outfile.write(json.dumps(self.to_serializable_dict()))
