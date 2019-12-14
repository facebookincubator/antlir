#!/usr/bin/env python3
'''
Given a `--socket-fd`, serves over HTTP a `--snapshot-dir` that was
previously produced by `snapshot-repos`.

Validates content checksums, so the specified `--storage` does not have to
be 100% trustworthy, we just need to trust the provenance of the
`--snapshot-dir`.

Here is how to run a test invocation of this server -- just be sure to use
the same `--storage` configuration as you did for your test snapshot:

  $ buck build //fs_image/rpm:repo-server
  $ python3 -c '
  import os, socket, subprocess, sys
  s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
  s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1) # if fixing a port
  s.bind(("127.0.0.1", 0))
  print("Socket bound - {}:{}".format(*s.getsockname()), file=sys.stderr)
  os.set_inheritable(s.fileno(), True)
  os.execlp(sys.argv[1], *sys.argv[1:], str(s.fileno()))
  ' buck-out/gen/fs_image/rpm/repo-server.par --storage \\
      '{"key": "test", "kind": "filesystem", "base_dir": "YOUR_PATH"}' \\
    --snapshot-dir YOUR_SNAPSHOT/ --socket-fd

'''
import json
import os
import socket
import sqlite3
import time
import urllib.parse

from socketserver import BaseServer
from http.server import BaseHTTPRequestHandler, HTTPStatus
from typing import Mapping, Tuple

from fs_image.common import get_file_logger, set_new_key
from fs_image.fs_utils import Path

from .common import Checksum
from .repo_objects import RepoMetadata
from .repo_snapshot import FileIntegrityError, ReportableError
from .storage import Storage

log = get_file_logger(__file__)

# How big are our reads against Storage? Exposed for the unit test.
_CHUNK_SIZE = 2 ** 21


# REVIEWERS: This OLD code path will be deleted later on this stack.
def read_snapshot_dir(path: Path):  # pragma: no cover
    if os.path.exists(path / 'snapshot.sql3'):
        return read_new_snapshot_dir(path)

    location_to_obj = {}
    for repo in os.listdir(path.decode()):
        if repo == 'yum.conf':
            continue
        repo_path = path / repo

        for filename in ['rpm.json', 'repodata.json']:
            with open(repo_path / filename) as infile:
                for location, obj in json.load(infile).items():
                    set_new_key(
                        location_to_obj, os.path.join(repo, location), obj,
                    )

        # Re-parse and serialize the metadata to a format that ALMOST
        # matches the other blobs (imitating `RepoSnapshot.to_directory()`).
        # If useful, it would not be offensive to make such a `repomd.json`
        # be emitted by RepoSnapshot, instead of `repomd.xml`.  Caveat: JSON
        # isn't suitable for bytes, and the XML is currently bytes.
        with open(repo_path / 'repomd.xml', 'rb') as infile:
            repomd = RepoMetadata.new(xml=infile.read())
        location_to_obj[os.path.join(repo, 'repodata/repomd.xml')] = {
            'size': repomd.size,
            'build_timestamp': repomd.build_timestamp,
            'content_bytes': repomd.xml,  # Instead of `storage_id`
        }

        # Similarly, make JSON metadata for the repo's GPG keys.
        key_dir = repo_path / 'gpg_keys'
        for key_filename in os.listdir(key_dir.decode()):
            with open(key_dir / key_filename, 'rb') as infile:
                key_content = infile.read()
            location_to_obj[os.path.join(repo, key_filename)] = {
                'size': len(key_content),
                # We don't have a good timestamp for these, so set it to
                # "now".  Caching efficiency losses should be negligible :)
                'build_timestamp': int(time.time()),
                'content_bytes': key_content,  # Instead of `storage_id`
            }

    return location_to_obj


# Future: we could query the RPM table lazily, which would save ~1 second of
# startup time for the FB production repo snapshot.
def add_snapshot_db_objs(db):
    location_to_obj = {}
    for repo, build_timestamp, metadata_xml in db.execute('''
    SELECT "repo", "build_timestamp", "metadata_xml" FROM "repomd"
    ''').fetchall():
        set_new_key(
            location_to_obj,
            os.path.join(repo, 'repodata/repomd.xml'),
            {
                'size': len(metadata_xml),
                'build_timestamp': build_timestamp,
                'content_bytes': metadata_xml.encode(),
            }
        )
    for table in ['repodata', 'rpm']:
        for (
            repo, path, build_timestamp, checksum, error, error_json, size,
            storage_id,
        ) in db.execute(f'''
        SELECT
            "repo", "path", "build_timestamp", "checksum", "error",
            "error_json", "size", "storage_id"
        FROM "{table}"
        ''').fetchall():
            obj = {
                'checksum': checksum,
                'size': size,
                'build_timestamp': build_timestamp,
            }
            # `storage_id` is populated in the DB table for `mutable_rpm`
            # errors, but we don't want to serve up those files.
            if storage_id and not error and not error_json:
                obj['storage_id'] = storage_id
            elif error and error_json:
                obj['error'] = {'error': error, **json.loads(error_json)}
            else:  # pragma: no cover
                raise AssertionError(f'{storage_id} {error} {error_json}')
            set_new_key(location_to_obj, os.path.join(repo, path), obj)
    return location_to_obj


def read_new_snapshot_dir(path: Path):
    db_path = path / 'snapshot.sql3'
    assert os.path.exists(db_path), f'no {db_path}, use rpm_repo_snapshot()'
    location_to_obj = add_snapshot_db_objs(sqlite3.connect(db_path))
    for repo in os.listdir(path / 'repos'):
        # Make JSON metadata for the repo's GPG keys.
        key_dir = path / 'repos' / repo / 'gpg_keys'
        for key_filename in os.listdir(key_dir.decode()):
            with open(key_dir / key_filename, 'rb') as infile:
                key_content = infile.read()
            location_to_obj[os.path.join(repo.decode(), key_filename)] = {
                'size': len(key_content),
                # We don't have a good timestamp for these, so set it to
                # "now".  Caching efficiency losses should be negligible :)
                'build_timestamp': int(time.time()),
                'content_bytes': key_content,  # Instead of `storage_id`
            }
    return location_to_obj


class RepoSnapshotHTTPRequestHandler(BaseHTTPRequestHandler):
    server_version = 'RPMRepoSnapshot'
    protocol_version = 'HTTP/1.0'

    def __init__(
        self, *args,
        # BEWARE: Mutated if we discover checksum errors to prevent client
        # retries from succeeding.
        location_to_obj: Mapping[str, dict],
        storage: Storage,
        **kwargs,
    ):
        self.location_to_obj = location_to_obj
        self.storage = storage
        super().__init__(*args, **kwargs)

    def _memoize_error(self, obj, error: ReportableError):
        '''
        Any size or checksum errors we see are likely to be permanent, so we
        MUTATE `obj` with the error, hiding the old `storage_id` inside.
        '''
        error_dict = {
            **error.to_dict(),
            # Since `storage_id` is hidden, `send_head` will show the error.
            'storage_id': obj.pop('storage_id'),
        }
        set_new_key(obj, 'error', error_dict)

    def do_GET(self) -> None:
        location, obj = self.send_head()
        if not obj:
            return  # Object not found, we already sent an error.
        if 'content_bytes' in obj:
            self.wfile.write(obj['content_bytes'])
            return

        # This binary blob must be fetched from `self.storage`. We don't
        # trust our storage, so we have to verify the checksum before
        # sending the entire blob back to the client.
        bytes_left = obj['size']
        checksum = Checksum.from_string(obj['checksum'])
        with self.storage.reader(obj['storage_id']) as input:
            hash = checksum.hasher()
            while True:
                chunk = input.read(_CHUNK_SIZE)
                bytes_left -= len(chunk)
                if not chunk:
                    if bytes_left != 0:  # The client will see an error.
                        self._memoize_error(obj, FileIntegrityError(
                            location=location,
                            failed_check='size',
                            expected=obj['size'],
                            actual=obj['size'] - bytes_left,
                        ))
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
                    self._memoize_error(obj, FileIntegrityError(
                        location=location,
                        failed_check='size',
                        expected=obj['size'],
                        actual=obj['size'] - bytes_left,
                    ))
                    break  # Incomplete content, client will see an error.

                hash.update(chunk)
                if bytes_left == 0 and hash.hexdigest() != checksum.hexdigest:
                    self._memoize_error(obj, FileIntegrityError(
                        location=location,
                        failed_check=checksum.algorithm,
                        expected=checksum.hexdigest,
                        actual=hash.hexdigest(),
                    ))
                    break  # Incomplete content, client will see an error.

                # If this is the last chunk, the stream was error-free.
                self.wfile.write(chunk)

    def do_HEAD(self):
        self.send_head()

    def send_head(self) -> Tuple[str, dict]:
        'Returns (location, obj) from the repo snapshot.'
        # Ignore query parameters & fragment, remove leading / if present.
        # Promoting to unicode since we get our repo snapshot from JSON, and
        # though ideally we'd use `unquote_to_bytes`.
        location = urllib.parse.unquote(
            urllib.parse.urlparse(self.path).path.lstrip('/'),
            'utf-8', 'surrogateescape',  # paper over invalid unicode :D
        )
        obj = self.location_to_obj.get(location)
        if obj is None:
            self.send_error(HTTPStatus.NOT_FOUND, 'File not found')
            return None, None
        if (
            ('storage_id' not in obj and 'content_bytes' not in obj) or
            # `error` is not currently populated simultaneously with
            # `storage_id`, but better safe than sorry.
            ('error' in obj)
        ):
            self.send_error(
                HTTPStatus.INTERNAL_SERVER_ERROR,
                f'Repo snapshot error: {obj.get("error")}',
            )
            # Future: we may add an option to grab the `storage_id` out of
            # 'mutable_rpm' errors, if appropriate.  Note that
            # `_memoize_error` hacks other errors to include a `storage_id`
            # in our in-memory representation -- do check the error type!
            return None, None
        self.send_response(HTTPStatus.OK)
        self.send_header('Content-type', self.type_for_path(location))
        self.send_header('Content-Length', str(obj['size']))
        self.send_header('Last-Modified', self.date_time_string(
            obj['build_timestamp'],
        ))
        # Future: if useful, could send_header('ETag', obj['checksum'])
        self.end_headers()
        return location, obj

    # There is also the more expensive & comprehensive `mimetypes` module,
    # but we don't need too many extensions.
    _EXTENSION_TO_MIME_TYPE = {
        'xml': 'text/xml',
        'gz': 'application/x-gzip',
        'bz2': 'application/x-bzip2',
        'rpm': 'application/x-rpm',
        # We could consider having drpm and srpm here, but they donf't seem
        # to have mime-types defined...
    }

    def type_for_path(self, path: str) -> str:
        parts = path.rsplit('.', 1)
        mimetype = self._EXTENSION_TO_MIME_TYPE.get(parts[-1].lower())
        if len(parts) < 2 or mimetype is None:
            return 'application/octet-stream'
        return mimetype


class HTTPSocketServer(BaseServer):
    '''
    A lightweight clone of the built-in HTTPServer & TCPServer to work
    around the fact that they do not accept pre-existing sockets.
    '''

    def __init__(self, sock: socket.socket, RequestHandlerClass):
        '''
        We just listen on `sock`. It may or may not be bound to any host or
        port **yet** -- and in fact, the binding will be done by another
        process on our behalf.
        '''
        # No server address since nothing actually needs to know it.
        super().__init__(None, RequestHandlerClass)
        self.socket = sock

    # This is only here as part of the BaseServer API, never to be run.
    def server_bind(self):  # pragma: no cover
        raise AssertionError(
            'self.socket must be bound externally before self.server_activate'
        )

    def server_activate(self):
        self.socket.listen()  # leaving the request queue size at default

    def server_close(self):
        self.socket.close()

    def fileno(self):
        return self.socket.fileno()

    def get_request(self):
        return self.socket.accept()

    def shutdown_request(self, request):
        try:
            # Explicitly shutdown -- `socket.close()` merely releases the
            # socket and waits for GC to perform the actual close.
            request.shutdown(socket.SHUT_WR)
        # This is cribbed from the Python standard library, but I have no
        # idea how to test it, hence the pragma.
        except OSError:  # pragma: no cover
            pass  # Some platforms may raise ENOTCONN here
        self.close_request(request)

    def close_request(self, request):
        request.close()


def repo_server(sock, location_to_obj: Mapping[str, dict], storage: Storage):
    '''
    BEWARE: `location_to_obj` is mutated if we discover checksum errors to
    prevent client retries from succeeding.
    '''
    return HTTPSocketServer(
        sock,
        lambda *args, **kwargs: RepoSnapshotHTTPRequestHandler(
            *args,
            location_to_obj=location_to_obj,
            storage=storage,
            **kwargs,
        )
    )


# Tested manually, as described in the file-level docblock.
if __name__ == '__main__':  # pragma: no cover
    import argparse

    from .common import init_logging

    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        '--snapshot-dir', required=True, type=Path.from_argparse,
        help='Multi-repo snapshot directory, with per-repo subdirectories, '
            'each containing repomd.xml, repodata.json, and rpm.json',
    )
    parser.add_argument(
        '--socket-fd', required=True, type=int,
        help='Listen on this socket. We assume that another process creates '
            'and binds the socket for us.',
    )
    Storage.add_argparse_arg(
        parser, '--storage', required=True,
        help='What Storage do the storage IDs of the snapshots refer to? ',
    )
    opts = parser.parse_args()

    init_logging()

    with repo_server(
        socket.socket(fileno=opts.socket_fd),
        read_snapshot_dir(opts.snapshot_dir),
        opts.storage,
    ) as httpd:
        httpd.server_activate()
        log.info(f'HTTP repo server is listening')
        httpd.serve_forever()
