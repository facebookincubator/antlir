#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Prints a pickled dict of test data to stdout. As a binary, does NOT work
in @mode/opt -- in that role, it's just a development tool.

The intent of this script is to exercise all the send-stream command types
that can be emitted by `btrfs send`.

After running from inside the per-Buck-repo btrfs volume,
`test_parse_dump.py` and `test_parse_send_stream.py` compare the parsed
output to what we expect on the basis of this script.

For usage, `buck run antlir/btrfs_diff:make-demo-sendstreams -- --help`.

## Updating this script's gold output

Run this:

  buck run antlir/btrfs_diff:make-demo-sendstreams -- \\
    --write-gold-to-dir antlir/btrfs_diff/tests/

You will then need to manually update `uuid_create` and related fields in
the "expected" section of the test.

In addition to parsing the gold output, `test_parse_dump.py` also checks
that we are able to parse the output of a **live** `btrfs receive --dump`.
Unfortunately, we are not able to check the **correctness** of these live
parses.  This is because the specific sequence of lines that `btrfs send`
produces to represent the filesystem is an implementation detail without a
_uniquely_ correct output, which may change over time.

Besides testing that parsing does not crash on a live `make_demo_sendstreams`,
whose output may even vary from host-to-host, we do two things:

 - Via this script, we freeze a sequence from one point in time just for the
   sake of having a parse-only test.

 - To test the semantics of the parsed data, we test applying a freshly
   generated sendstream to a mock filesystem, which should always give the
   same result, regardless of the specific send-stream commands used.
"""
import argparse
import contextlib
import enum

#
# Note: When used as a library (through functions not prefixed with
# underscore), this code needs to work in @mode/opt, and so should not
# assume it has unfettered access to the source repo.
#
import os
import pickle
import pprint
import shlex
import subprocess
import sys
import time
from typing import Tuple

from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol, TempSubvolumes


def _make_create_ops_subvolume(subvols: TempSubvolumes, path: bytes) -> Subvol:
    "Exercise all the send-stream ops that can occur on a new subvolume."
    subvol = subvols.create(path)
    run = subvol.run_as_root

    # `cwd` is intentionally prohibited with `run_as_root`
    def p(sv_path):
        return subvol.path(sv_path).decode()

    # Due to an odd `btrfs send` implementation detail, creating a file or
    # directory emits a rename from a temporary name to the final one.
    run(["mkdir", p("hello")])  # mkdir,rename
    run(["mkdir", p("dir_to_remove")])
    run(["touch", p("hello/world")])  # mkfile,utimes,chmod,chown
    run(
        [  # set_xattr
            "setfattr",
            "-n",
            "user.test_attr",
            "-v",
            "chickens",
            p("hello/"),
        ]
    )
    run(["mknod", p("buffered"), "b", "42", "31"])  # mknod
    run(["chmod", "og-r", p("buffered")])  # chmod a device
    run(["mknod", p("unbuffered"), "c", "42", "31"])
    run(["mkfifo", p("fifo")])  # mkfifo
    run(
        [
            "python3",
            "-c",
            (
                "import os, sys, socket as s\n"
                "dir, base = os.path.split(sys.argv[1])\n"
                # Otherwise, we can easily get "AF_UNIX path too long"
                'os.chdir(os.path.join(".", dir))\n'
                "with s.socket(s.AF_UNIX, s.SOCK_STREAM) as sock:\n"
                "    sock.bind(base)\n"  # mksock
            ),
            p("unix_sock"),
        ]
    )
    run(["ln", p("hello/world"), p("goodbye")])  # link
    run(["ln", "-s", "hello/world", p("bye_symlink")])  # symlink
    run(
        [  # update_extent
            # 56KB was chosen so that `btrfs send` emits more than 1 write,
            # specifically 48KB + 8KB.
            "dd",
            "if=/dev/zero",
            "of=" + p("56KB_nuls"),
            "bs=1024",
            "count=56",
        ]
    )
    run(
        [  # clone
            "cp",
            "--reflink=always",
            p("56KB_nuls"),
            p("56KB_nuls_clone"),
        ]
    )

    # Make a file with a 16KB hole in the middle.
    run(
        [
            "dd",
            "if=/dev/zero",
            "of=" + p("zeros_hole_zeros"),
            "bs=1024",
            "count=16",
        ]
    )
    run(["truncate", "-s", str(32 * 1024), p("zeros_hole_zeros")])
    run(
        [
            "dd",
            "if=/dev/zero",
            "of=" + p("zeros_hole_zeros"),
            "oflag=append",
            "conv=notrunc",
            "bs=1024",
            "count=16",
        ]
    )
    # A trailing hole exercises the `truncate` sendstream command.
    run(["bash", "-c", "echo hello > " + shlex.quote(p("hello_big_hole"))])
    run(["truncate", "-s", "1G", p("hello_big_hole")])

    # We expect our create_ops image to have selinux attributes set. On
    # some systems there are created by default, but not always. So set
    # some explicitly here to guarantee we always have at least one.
    run(["touch", p("selinux_xattrs")])
    run(
        [  # set_xattr
            "sudo",
            "setfattr",
            "-n",
            "security.selinux",
            "-v",
            "user_u:object_r:base_t",
            p("selinux_xattrs"),
        ]
    )

    # Create a directory with perms 0500
    run(["mkdir", p("dir_perms_0500")])
    run(["chmod", "0500", p("dir_perms_0500")])

    # Create a directory and file owned and only accessible by another user
    run(["mkdir", p("user1")])
    run(["chown", "1:1", p("user1")])
    run(["chmod", "0700", p("user1")])
    run(["touch", p("user1/data")])
    run(["chown", "1:1", p("user1/data")])
    run(["chmod", "0400", p("user1/data")])

    # Create a directory with ACLs
    # ACLs copied from /run/log/journal on CentOS 8
    run(["mkdir", p("dir_with_acls")])
    run(
        [
            "setfacl",
            "-m",
            "group:adm:r-x",
            "-m",
            "group:wheel:r-x",
            "-m",
            "mask::r-x",
            "-m",
            "default:user::rwx",
            "-m",
            "default:group::r-x",
            "-m",
            "default:group:adm:r-x",
            "-m",
            "default:group:wheel:r-x",
            "-m",
            "default:mask::r-x",
            "-m",
            "default:other::r-x",
            p("dir_with_acls"),
        ]
    )

    # This just serves to show that `btrfs send` ignores nested subvolumes.
    # There is no mention of `nested_subvol` in the send-stream.
    nested_subvol = Subvol(subvol.path("nested_subvol")).create()
    nested_subvol.run_as_root(["touch", nested_subvol.path("borf")])
    nested_subvol.run_as_root(["mkdir", nested_subvol.path("beep")])

    return subvol


def _make_mutate_ops_subvolume(
    subvols: TempSubvolumes, create_ops: Subvol, path: bytes
) -> Subvol:
    "Exercise the send-stream ops that are unique to snapshots."
    subvol = subvols.snapshot(create_ops, path)  # snapshot
    run = subvol.run_as_root

    # `cwd` is intentionally prohibited with `run_as_root`
    def p(sv_path):
        return subvol.path(sv_path).decode()

    run(["rm", p("hello/world")])  # unlink
    run(["rmdir", p("dir_to_remove/")])  # rmdir
    run(["setfattr", "--remove=user.test_attr", p("hello/")])  # remove_xattr
    # You would think this would emit a `rename`, but for files, the
    # sendstream instead `link`s to the new location, and unlinks the old.
    run(["mv", p("goodbye"), p("farewell")])  # NOT a rename, {,un}link
    run(["mv", p("hello/"), p("hello_renamed/")])  # yes, a rename!
    run(["dd", "of=" + p("hello_renamed/een")], input=b"push\n")  # write
    # This is a no-op because `btfs send` does not support `chattr` at
    # present.  However, it's good to have a canary so that our tests start
    # failing the moment it is supported -- that will remind us to update
    # the mock VFS.  NB: The absolute path to `chattr` is a clowny hack to
    # work around a clowny hack, to work around clowny hacks.  Don't ask.
    run(["/usr/bin/chattr", "+Ac", p("hello_renamed/een")])
    # Besides files with trailing holes, one can also get `truncate`
    # sendstream commands in incremental sendstreams by having a snapshot
    # truncate relative a file relative to the parent.
    run(["truncate", "-s", "2", p("hello_big_hole")])

    return subvol


def _float_to_sec_nsec_tuple(t: float) -> Tuple[int, int]:
    sec = int(t)
    return (sec, int(1e9 * (t - sec)))


@contextlib.contextmanager
def _populate_sendstream_dict(d):
    d["build_start_time"] = _float_to_sec_nsec_tuple(time.time())
    yield d
    d["dump"] = (
        subprocess.run(
            ["btrfs", "receive", "--dump"],
            input=d["sendstream"],
            stdout=subprocess.PIPE,
            check=True,
            # split into lines to make the `pretty` output prettier
        )
        .stdout.rstrip(b"\n")
        .split(b"\n")
    )
    d["build_end_time"] = _float_to_sec_nsec_tuple(time.time())


# Takes `path_in_repo` because this is part of the library interface, and
# thus must work in @mode/opt, and thus we cannot use `__file__` here.
def make_demo_sendstreams(path_in_repo: Path):
    with TempSubvolumes(path_in_repo) as subvols:
        res = {}

        with _populate_sendstream_dict(res.setdefault("create_ops", {})) as d:
            create_ops = _make_create_ops_subvolume(subvols, b"create_ops")
            d["sendstream"] = create_ops.mark_readonly_and_get_sendstream()

        with _populate_sendstream_dict(res.setdefault("mutate_ops", {})) as d:
            d["sendstream"] = _make_mutate_ops_subvolume(
                subvols, create_ops, b"mutate_ops"
            ).mark_readonly_and_get_sendstream(
                parent=create_ops,
                # The resulting send-stream will have `update_extent`
                # instead of `write`, which is one way of making sure that
                # `update_extent` in `parse_sendstream.py` is covered.
                no_data=True,
            )

        return res


def _main() -> None:
    p = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )

    class Print(enum.Enum):
        NONE = "none"
        PRETTY = "pretty"
        PICKLE = "pickle"

        def __str__(self):
            return self.value

    p.add_argument(
        "--print",
        type=Print,
        choices=Print,
        default=Print.NONE,
        help="If set, prints the result in the specified format to stdout.",
    )
    p.add_argument(
        "--write-gold-to-dir",
        help="If set, writes the gold test data into the given directory as "
        "gold_demo_sendstreams.{pickle,pretty}. Warning: you will need "
        "to manually update some constants like `uuid_create` in the "
        '"expected" section of the test code.',
    )
    args = p.parse_args()

    # __file__ won't let us find the repo in @mode/opt, but that's OK, since
    # this is only used as a binary for development purposes.
    sendstream_dict = make_demo_sendstreams(Path(__file__))

    # This width makes the `--dump`ed commands fit on one line.
    prettified = pprint.pformat(sendstream_dict, width=200).encode()
    pickled = pickle.dumps(sendstream_dict)

    if args.print == Print.PRETTY:
        sys.stdout.buffer.write(prettified)
    elif args.print == Print.PICKLE:
        sys.stdout.buffer.write(pickled)
    else:
        assert args.print == Print.NONE, args.print

    if args.write_gold_to_dir is not None:
        for filename, data in [
            ("gold_demo_sendstreams.pickle", pickled),
            ("gold_demo_sendstreams.pretty", prettified),  # For humans
        ]:
            path = os.path.join(args.write_gold_to_dir, filename)
            # We want these files to be created by a non-root user
            assert os.path.exists(path), path
            with open(path, "wb") as f:
                f.write(data)


def gold_demo_sendstreams():
    with Path.resource(
        __package__, "gold_demo_sendstreams.pickle", exe=False
    ) as pickle_path, open(pickle_path, "rb") as f:
        return pickle.load(f)


if __name__ == "__main__":
    _main()
