#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Given a `--socket-fd`, serves over HTTP a `--snapshot-dir` that was
previously produced by `snapshot-repos`.

Validates content checksums, so the snapshot's blobstore does not have to be
100% trustworthy, we just need to trust the provenance of the
`--snapshot-dir`.

Here is how to run a test invocation of this server:

  $ buck build //antlir/rpm:repo-server
  $ python3 -c '
  import os, socket, subprocess, sys
  s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
  s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1) # if fixing a port
  s.bind(("127.0.0.1", 0))
  print("Socket bound - {}:{}".format(*s.getsockname()), file=sys.stderr)
  os.set_inheritable(s.fileno(), True)
  os.execlp(sys.argv[1], *sys.argv[1:], "--socket-fd", str(s.fileno()))
  ' buck-out/gen/antlir/rpm/repo-server.par --snapshot-dir YOUR_SNAPSHOT/

"""
import json
import os
import socket
import time
import urllib.parse

# pyre-fixme[21]: Could not find name `HTTPStatus` in `http.server`.
from http.server import BaseHTTPRequestHandler, HTTPStatus
from typing import Mapping, Tuple

from antlir.common import get_logger, init_logging, set_new_key
from antlir.fs_utils import Path
from antlir.proxy.http_socket_server import HTTPSocketServer

from .common import Checksum, readonly_snapshot_db, snapshot_subdir
from .repo_snapshot import FileIntegrityError, ReportableError
from .storage import Storage


log = get_logger()

# How big are our reads against Storage? Exposed for the unit test.
_CHUNK_SIZE = 2**21


# Future: we could query the RPM table lazily, which would save ~1 second of
# startup time for the FB production repo snapshot.
def add_snapshot_db_objs(db):
    location_to_obj = {}
    for repo, build_timestamp, metadata_xml in db.execute(
        """
    SELECT "repo", "build_timestamp", "metadata_xml" FROM "repomd"
    """
    ).fetchall():
        set_new_key(
            location_to_obj,
            os.path.join(repo, "repodata/repomd.xml"),
            {
                "size": len(metadata_xml),
                "build_timestamp": build_timestamp,
                "content_bytes": metadata_xml.encode(),
            },
        )
    for table in ["repodata", "rpm"]:
        for (
            repo,
            path,
            build_timestamp,
            checksum,
            error,
            error_json,
            size,
            storage_id,
        ) in db.execute(
            f"""
        SELECT
            "repo", "path", "build_timestamp", "checksum", "error",
            "error_json", "size", "storage_id"
        FROM "{table}"
        """
        ).fetchall():
            obj = {
                "checksum": checksum,
                "size": size,
                "build_timestamp": build_timestamp,
            }
            # `storage_id` is populated in the DB table for `mutable_rpm`
            # errors, but we don't want to serve up those files.
            if storage_id and not error and not error_json:
                obj["storage_id"] = storage_id
            elif error and error_json:
                obj["error"] = {"error": error, **json.loads(error_json)}
            else:  # pragma: no cover
                raise AssertionError(f"{storage_id} {error} {error_json}")
            set_new_key(location_to_obj, os.path.join(repo, path), obj)
    return location_to_obj


def read_snapshot_dir(snapshot_dir: Path):
    with readonly_snapshot_db(snapshot_dir) as db:
        location_to_obj = add_snapshot_db_objs(db)
    repos_dir = snapshot_subdir(snapshot_dir) / "repos"
    for repo in repos_dir.listdir():
        # Make JSON metadata for the repo's GPG keys.
        key_dir = repos_dir / repo / "gpg_keys"
        for key_filename in key_dir.listdir():
            with open(key_dir / key_filename, "rb") as infile:
                key_content = infile.read()
            location_to_obj[(repo / key_filename).decode()] = {
                "size": len(key_content),
                # We don't have a good timestamp for these, so set it to
                # "now".  Caching efficiency losses should be negligible :)
                "build_timestamp": int(time.time()),
                "content_bytes": key_content,  # Instead of `storage_id`
            }
    return location_to_obj


class RepoSnapshotHTTPRequestHandler(BaseHTTPRequestHandler):
    server_version = "RPMRepoSnapshot"
    protocol_version = "HTTP/1.0"

    def __init__(
        self,
        *args,
        location_to_obj: Mapping[str, dict],
        storage: Storage,
        **kwargs,
    ) -> None:
        self.location_to_obj = location_to_obj
        self.storage = storage
        super().__init__(*args, **kwargs)

    def _memoize_error(self, obj, error: ReportableError) -> None:
        """
        Any size or checksum errors we see are likely to be permanent, so we
        MUTATE `obj` with the error, hiding the old `storage_id` inside.
        """
        error_dict = {
            **error.to_dict(),
            # Since `storage_id` is hidden, `send_head` will show the error.
            "storage_id": obj.pop("storage_id"),
        }
        set_new_key(obj, "error", error_dict)

    # The default logging implementation does not flush. Gross.
    def log_message(self, format: str, *args, _antlir_logger=log.debug) -> None:
        _antlir_logger(
            "%s - - [%s] %s\n"
            % (
                self.address_string(),
                self.log_date_time_string(),
                format % args,
            )
        )

    def log_error(self, format: str, *args) -> None:
        # `repo-server` errors should be visible when e.g.  `yum` or `dnf`
        # are running in a default `buck run :foo=container`.
        self.log_message(format, *args, _antlir_logger=log.warning)

    def do_GET(self) -> None:
        location, obj = self.send_head()
        if not obj:
            return  # Object not found, we already sent an error.
        if "content_bytes" in obj:
            self.wfile.write(obj["content_bytes"])
            return

        # This binary blob must be fetched from `self.storage`. We don't
        # trust our storage, so we have to verify the checksum before
        # sending the entire blob back to the client.
        bytes_left = obj["size"]
        checksum = Checksum.from_string(obj["checksum"])
        # pyre-fixme[16]: `Storage` has no attribute `reader`.
        with self.storage.reader(obj["storage_id"]) as input:
            log.debug(f"Got storage for {location}")
            hash = checksum.hasher()
            while True:
                chunk = input.read(_CHUNK_SIZE)
                log.debug(f"{len(chunk)}-byte chunk for {location}")
                bytes_left -= len(chunk)
                if not chunk:
                    if bytes_left != 0:  # The client will see an error.
                        self._memoize_error(
                            obj,
                            FileIntegrityError(
                                location=location,
                                failed_check="size",
                                expected=obj["size"],
                                actual=obj["size"] - bytes_left,
                            ),
                        )
                    break

                #
                # Check for errors **before** sending out more data -- this
                # might be the last chunk, and so we signal errors by
                # refusing to send the last bit of data.
                #

                # It's possible that we have a chunk after the last chunk,
                # but we don't want to send that last chunk since the client
                # might conclude all is well upon receiving enough data.
                if bytes_left == 0:
                    # The next `if` will error if we get a non-empty chunk.
                    # The error's `actual=` might be an underestimate.
                    bytes_left -= len(input.read())

                if bytes_left < 0:
                    self._memoize_error(
                        obj,
                        FileIntegrityError(
                            location=location,
                            failed_check="size",
                            expected=obj["size"],
                            actual=obj["size"] - bytes_left,
                        ),
                    )
                    break  # Incomplete content, client will see an error.

                hash.update(chunk)
                if bytes_left == 0 and hash.hexdigest() != checksum.hexdigest:
                    self._memoize_error(
                        obj,
                        FileIntegrityError(
                            location=location,
                            failed_check=checksum.algorithm,
                            expected=checksum.hexdigest,
                            actual=hash.hexdigest(),
                        ),
                    )
                    break  # Incomplete content, client will see an error.

                # If this is the last chunk, the stream was error-free.
                self.wfile.write(chunk)

        log.debug(f"Normal exit for GET {location}")

    def do_HEAD(self) -> None:
        self.send_head()

    def send_head(self) -> Tuple[str, dict]:
        "Returns (location, obj) from the repo snapshot."
        # Ignore query parameters & fragment, remove leading / if present.
        # Promoting to unicode since because historically our repo paths
        # have been strings, and though ideally we'd update
        # `RepoSnapshot.to_sqlite` and use `unquote_to_bytes`.
        location = urllib.parse.unquote(
            urllib.parse.urlparse(self.path).path.lstrip("/"),
            "utf-8",
            "surrogateescape",  # paper over invalid unicode :D
        )
        log.debug(f"HEAD {location}")
        obj = self.location_to_obj.get(location)
        if obj is None:
            # pyre-fixme[16]: Module `server` has no attribute `HTTPStatus`.
            self.send_error(HTTPStatus.NOT_FOUND, "File not found")
            # pyre-fixme[7]: Expected `Tuple[str, typing.Dict[typing.Any,
            #  typing.Any]]` but got `Tuple[None, None]`.
            return None, None
        if (
            ("storage_id" not in obj and "content_bytes" not in obj)
            # `error` is not currently populated simultaneously with
            # `storage_id`, but better safe than sorry.
            or ("error" in obj)
        ):
            self.send_error(
                # pyre-fixme[16]: Module `server` has no attribute `HTTPStatus`.
                HTTPStatus.INTERNAL_SERVER_ERROR,
                f'Repo snapshot error: {obj.get("error")}',
            )
            # Future: we may add an option to grab the `storage_id` out of
            # 'mutable_rpm' errors, if appropriate.  Note that
            # `_memoize_error` hacks other errors to include a `storage_id`
            # in our in-memory representation -- do check the error type!
            # pyre-fixme[7]: Expected `Tuple[str, typing.Dict[typing.Any,
            #  typing.Any]]` but got `Tuple[None, None]`.
            return None, None
        # pyre-fixme[16]: Module `server` has no attribute `HTTPStatus`.
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-type", self.type_for_path(location))
        self.send_header("Content-Length", str(obj["size"]))
        self.send_header(
            "Last-Modified", self.date_time_string(obj["build_timestamp"])
        )
        # Future: if useful, could send_header('ETag', obj['checksum'])
        self.end_headers()
        return location, obj

    # There is also the more expensive & comprehensive `mimetypes` module,
    # but we don't need too many extensions.
    _EXTENSION_TO_MIME_TYPE = {
        "xml": "text/xml",
        "gz": "application/x-gzip",
        "bz2": "application/x-bzip2",
        "rpm": "application/x-rpm",
        # We could consider having drpm and srpm here, but they donf't seem
        # to have mime-types defined...
    }

    def type_for_path(self, path: str) -> str:
        parts = path.rsplit(".", 1)
        mimetype = self._EXTENSION_TO_MIME_TYPE.get(parts[-1].lower())
        if len(parts) < 2 or mimetype is None:
            return "application/octet-stream"
        return mimetype


def repo_server(
    sock, location_to_obj: Mapping[str, dict], storage: Storage
) -> HTTPSocketServer:
    """
    BEWARE: `location_to_obj` is mutated if we discover checksum errors to
    prevent client retries from succeeding.
    """
    return HTTPSocketServer(
        sock,
        lambda *args, **kwargs: RepoSnapshotHTTPRequestHandler(
            *args, location_to_obj=location_to_obj, storage=storage, **kwargs
        ),
    )


# Tested manually, as described in the file-level docblock.
def main() -> None:  # pragma: no cover
    import argparse

    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--snapshot-dir",
        required=True,
        type=Path.from_argparse,
        help="Multi-repo snapshot directory, with per-repo subdirectories, "
        "each containing repomd.xml, repodata.json, and rpm.json",
    )
    parser.add_argument(
        "--socket-fd",
        required=True,
        type=int,
        help="Listen on this socket. We assume that another process creates, "
        " binds, and (optionally) listens on the socket for us.",
    )
    parser.add_argument("--debug", action="store_true", help="Log more")
    args = parser.parse_args()
    init_logging(debug=args.debug)

    # TODO: Once all BAs include a repo-server with the new code that knows
    # to append "/snapshot", remove this & tidy `launch_repo_servers.py`.
    if not snapshot_subdir(args.snapshot_dir).exists():
        assert args.snapshot_dir.basename() == b"snapshot", args.snapshot_dir
        args.snapshot_dir = args.snapshot_dir.dirname()

    with open(snapshot_subdir(args.snapshot_dir) / "storage.json") as f:
        storage = json.load(f)
    # We want relative `base_dir` to point into the snapshot dir.
    if storage["kind"] == "filesystem":
        storage["base_dir"] = (
            snapshot_subdir(args.snapshot_dir) / storage["base_dir"]
        ).normpath()

    with repo_server(
        socket.socket(fileno=args.socket_fd),
        read_snapshot_dir(args.snapshot_dir),
        # pyre-fixme[6]: For 3rd param expected `Storage` but got `Pluggable`.
        Storage.from_json(storage),
    ) as httpd:
        # In the current usage, we start listening in `_launch_repo_server`,
        # but leaving this here should be harmless.
        httpd.server_activate()
        snapshot_short_name = args.snapshot_dir.basename()
        log.debug(f"HTTP `repo-server` for {snapshot_short_name} is listening")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:  # pragma: no cover
            log.debug("HTTP `repo-server` graceful shutdown on SIGINT")
