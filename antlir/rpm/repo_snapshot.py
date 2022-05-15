#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
If you have an `rpm_repo_snapshot` target named //foo:bar, you can
tally all `ReportableError`s via:

    echo '
    SELECT "error", COUNT(1) FROM "rpm" WHERE "error" IS NOT NULL
    GROUP BY "error";
    ' | sqlite3 file://"$(readlink -f "$(
      buck build //foo:bar --show-full-output | cut -f 2 -d ' '
    )")"/snapshot.sql3?mode=ro
"""
import json
import sqlite3
import sys
import tempfile
from contextlib import contextmanager
from typing import Any, Callable, Iterable, Mapping, NamedTuple, Union

from antlir.common import get_logger
from antlir.fs_utils import create_ro, Path

from .common import read_chunks
from .repo_objects import Repodata, RepoMetadata, Rpm
from .storage import Storage


log = get_logger()

# Places making this assumption should be findable by the string "3.7"
assert sys.hexversion >= 0x030700F0, "This relies on dicts being ordered."


class ReportableError(Exception):
    """
    Base class for errors do not abort the snapshot, but have to be reported
    as part of RepoSnapshot.
    """

    def __init__(self, **kwargs) -> None:
        # Even though this is morally a dict, writing a tuple to `args`
        # better honors Python's interesting exception norms, and gets us
        # usable-looking backtraces for "free".
        self.args = tuple(sorted(kwargs.items()))
        log.error(f"{type(self).__name__}{self.args}")

    def to_dict(self):
        """
        Returns a POD dictionary to be output as an `'error'` field in the
        serialized `RepoSnapshot`.
        """
        return dict(self.args)


class FileIntegrityError(ReportableError):
    def __init__(self, *, location, failed_check, expected, actual) -> None:
        super().__init__(
            error="file_integrity",
            message="File had unexpected size or checksum",
            location=location,
            failed_check=failed_check,
            expected=str(expected),
            actual=str(actual),
        )


class HTTPError(ReportableError):
    def __init__(self, *, location, http_status) -> None:
        super().__init__(
            error="http",
            message="Failed HTTP request while downloading a repo object",
            location=location,
            http_status=http_status,
        )


class MutableRpmError(ReportableError):
    def __init__(
        self, *, location, storage_id, checksum, other_checksums_and_universes
    ) -> None:
        super().__init__(
            error="mutable_rpm",
            message="Found MULTIPLE canonical checksums for one RPM NEVRA. "
            "This means that the same RPM exists (or had existed) with "
            "different variants of its content.",
            location=location,
            storage_id=storage_id,  # This bad RPM is still retrievable
            checksum=str(checksum),  # Canonical checksum of this `storage_id`
            # Canonical checksums with other storage IDs & the same NEVRA:
            other_checksums_and_universes=sorted(
                (str(c), u) for c, u in other_checksums_and_universes
            ),
        )


MaybeStorageID = Union[str, ReportableError]


class RepoSnapshot(NamedTuple):
    repomd: RepoMetadata
    storage_id_to_repodata: Mapping[MaybeStorageID, Repodata]
    storage_id_to_rpm: Mapping[MaybeStorageID, Rpm]

    _RPM_COLUMNS = {  # We're on 3.7+, so this dict is ordered
        "repo": "TEXT NOT NULL",
        "path": "TEXT NOT NULL",
        "name": "TEXT NOT NULL",
        "version": "TEXT NOT NULL",
        "release": "TEXT NOT NULL",
        "epoch": "INTEGER NOT NULL",
        "arch": "TEXT NOT NULL",
        "build_timestamp": "INTEGER NOT NULL",
        "checksum": "TEXT NOT NULL",
        "error": "TEXT",
        "error_json": "TEXT",
        "size": "INTEGER NOT NULL",
        # Repos can also contain source RPMs, for which this is NULL.
        # `repodata_downloader.py` checks that only `.src.rpm`s do this.
        "source_rpm": "TEXT",
        "storage_id": "TEXT",
    }

    _REPODATA_COLUMNS = {  # We're on 3.7+, so this dict is ordered
        "repo": "TEXT NOT NULL",
        "path": "TEXT NOT NULL",
        "build_timestamp": "INTEGER NOT NULL",
        "checksum": "TEXT NOT NULL",
        "error": "TEXT",
        "error_json": "TEXT",
        "size": "INTEGER NOT NULL",
        "storage_id": "TEXT",
    }

    _REPOMD_COLUMNS = {  # We're on 3.7+, so this dict is ordered
        "repo": "TEXT NOT NULL",
        "build_timestamp": "INTEGER NOT NULL",
        "metadata_xml": "TEXT NOT NULL",
    }

    @classmethod
    def _create_sqlite_tables(cls, db: sqlite3.Connection) -> None:
        # For `repo_server.py` we need repo + path lookup, so that's the
        # primary key.
        #
        # For repo debugging & exploration, we want global lookup on
        # name-version-release -- hence the `nvrea` index.  It's unimportant
        # to index on arch & epoch, or not to index on repo, since the total
        # number of rows for a given NVR should be low.
        db.executescript(
            """
        CREATE TABLE "rpm" ({rpm_cols}, PRIMARY KEY ("repo", "path"));
        CREATE INDEX "rpm_nvrea" ON "rpm" (
            "name", "version", "release", "epoch", "arch"
        );
        CREATE TABLE "repodata" ({repodata_cols}, PRIMARY KEY ("repo", "path"));
        CREATE TABLE "repomd" ({repomd_cols}, PRIMARY KEY ("repo"));
        """.format(
                **{
                    f"{table}_cols": ",\n".join(
                        f'"{k}" {v}' for k, v in col_spec.items()
                    )
                    for table, col_spec in [
                        ("rpm", cls._RPM_COLUMNS),
                        ("repodata", cls._REPODATA_COLUMNS),
                        ("repomd", cls._REPOMD_COLUMNS),
                    ]
                }
            )
        )

    _STORAGE_CHUNK_SIZE = 2**20  # Anything that's not too small is OK.
    _STORAGE_ID_FILE = "snapshot.storage_id"

    @classmethod
    @contextmanager
    def add_sqlite_to_storage(
        cls, storage: Storage, dest_dir: Path
    ) -> Iterable[sqlite3.Connection]:
        with tempfile.NamedTemporaryFile() as db_tf:
            with sqlite3.connect(db_tf.name) as db:
                RepoSnapshot._create_sqlite_tables(db)
                yield db
            db.close()
            # pyre-fixme[16]: `Storage` has no attribute `writer`.
            with storage.writer() as db_out:
                # pyre-fixme[6]: Expected `BytesIO` for 1st param but got
                #  `_TemporaryFileWrapper[bytes]`.
                for chunk in read_chunks(db_tf, cls._STORAGE_CHUNK_SIZE):
                    db_out.write(chunk)
                with create_ro(dest_dir / cls._STORAGE_ID_FILE, "w") as sidf:
                    sidf.write(db_out.commit() + "\n")

    @classmethod
    def fetch_sqlite_from_storage(
        cls, storage: Storage, from_dir: Path, dest: Path
    ) -> Path:
        """
        At present, this is just a helper for tests. Real builds should
        use `rpm_repo_snapshot()` to fetch the `.sql3` DB

        Returns the populated `dest` for convenience.
        """
        with open(from_dir / cls._STORAGE_ID_FILE) as sid_in:
            sid = sid_in.read()
            assert sid[-1] == "\n", repr(sid)
            sid = sid[:-1]
        # pyre-fixme[16]: `Storage` has no attribute `reader`.
        with storage.reader(sid) as db_in, create_ro(dest, "wb") as db_out:
            for chunk in read_chunks(db_in, cls._STORAGE_CHUNK_SIZE):
                db_out.write(chunk)
        return dest

    def _gen_object_rows(
        self,
        repo: str,
        sid_to_obj: Union[
            Mapping[MaybeStorageID, Union[Rpm]],
            Mapping[MaybeStorageID, Union[Repodata]],
        ],
        expected_columns: Iterable[str],
        get_other_cols_fn: Union[
            Callable[[Rpm], Mapping[str, Any]],
            Callable[[Repodata], Mapping[str, Any]],
        ],
    ):
        for sid, obj in sid_to_obj.items():
            if isinstance(sid, ReportableError):
                error_dict = sid.to_dict()
                error = error_dict.pop("error")
                sid = error_dict.pop("storage_id", None)
            else:
                error_dict = None
                error = None
            d = {
                "repo": repo,
                "path": obj.location,
                "build_timestamp": obj.build_timestamp,
                "checksum": str(obj.best_checksum()),
                "error": error,
                "error_json": json.dumps(error_dict, sort_keys=True)
                if error_dict
                else None,
                "size": obj.size,
                "storage_id": sid,
            }
            # pyre-fixme[6]: Expected `Repodata` for 1st param but got
            #  `Union[Repodata, Rpm]`.
            other_d = get_other_cols_fn(obj)
            assert not (set(d) & set(other_d)), (d, other_d)
            d.update(other_d)
            assert set(d) == set(expected_columns), (d, expected_columns)
            yield d

    def to_sqlite(self, repo: str, db: sqlite3.Connection) -> None:
        for table, columns, gen_rows in [
            (
                "rpm",
                self._RPM_COLUMNS,
                self._gen_object_rows(
                    repo,
                    self.storage_id_to_rpm,
                    self._RPM_COLUMNS,
                    lambda rpm: {
                        "name": rpm.name,
                        "version": rpm.version,
                        "release": rpm.release,
                        "epoch": rpm.epoch,
                        "arch": rpm.arch,
                        "source_rpm": rpm.source_rpm,
                    },
                ),
            ),
            (
                "repodata",
                self._REPODATA_COLUMNS,
                self._gen_object_rows(
                    repo,
                    self.storage_id_to_repodata,
                    self._REPODATA_COLUMNS,
                    lambda repodata: {},
                ),
            ),
            (
                "repomd",
                self._REPOMD_COLUMNS,
                [
                    {
                        "repo": repo,
                        "build_timestamp": self.repomd.build_timestamp,
                        "metadata_xml": self.repomd.xml.decode(),
                    }
                ],
            ),
        ]:
            db.executemany(
                'INSERT INTO {} ("{}") VALUES ({});'.format(
                    table, '", "'.join(columns), ", ".join(["?"] * len(columns))
                ),
                ([d[k] for k in columns] for d in gen_rows),
            )

    def visit(self, visitor) -> "RepoSnapshot":
        "Visits the objects in this snapshot (i.e. this shard)"
        visitor.visit_repomd(self.repomd)
        for repodata in self.storage_id_to_repodata.values():
            visitor.visit_repodata(repodata)
        for rpm in self.storage_id_to_rpm.values():
            visitor.visit_rpm(rpm)
        return self
