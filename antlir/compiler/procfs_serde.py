#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
## Why use a `proc`-like format for container metadata?

On Linux `/proc` is a rich database of kernel data, most of it easily
accessible to standard shell scripts. By using the filesystem interface,
the data is also made easy for humans to navigate and discover.

For legacy reasons, the `procfs` interface is neither perfectly consistent,
nor perfectly usable, but its core idea is nice.

In image construction, we have a similar need to record image / container
metadata as part of the filesystem.

This file here formalizes a `procfs`-inspired filesystem serialization
format for "plain old data" -- the usual combinations of dicts, lists, and
scalar types.

Such plain old data could equally well be serialized to a single JSON file
by using `surrogateescapes` for non-unicode strings like filesystem paths
and binary data.  However, using JSON this way has some downsides:
  - It is nonstandard -- `surrogateescape` for JSON binary data is only
    really usable in Python, not in `jq` or other common JSON tools.
  - Scripting with JSON is laborious, unless you have `jq`.  And `jq` would
    not decode `surrogateescape`.
  - One keeps re-reading and re-parsing and re-writing the whole JSON file,
    even if one just needs a single item.  This is unwieldy and race-prone,
    no matter what the relative performance of btrfs vs JSON is in practice.
  - The filesystem provides nice freebies like granular access control &
    automatic timestamps.


## A formal definition of our format

Data is arranged in a directory hierarchy.  We currently use a single
regular file for an atom of data.

Directories must currently not have an extension.  They correspond to dicts
in the natural way.  We will support for lists-as-directories when needed.

The type of a data file is determined by its extension:

  - No extension -- a single UTF-8 string.  All non-NULL characters are
    permitted, including newlines.  The serialization always appends a
    trailing newline, because this makes `cat *_path` behave sanely, and is
    concordant with how `bash` handles command substitution in `$()`.

  - `.image_path` / `.host_path` -- a path inside the container / on the
    outer host. Details:
      - UTF-8 is not guaranteed, this is a Linux path, not a Unicode string.
      - This is a regular file and NOT a symlink like `procfs` would present.
      - As for "no extension", a trailing newline is always added.
    Aside: Accessing host paths is never a first-choice solution, and should
    be avoided if possible.  However, sometimes they are a valid
    intermediate solution on the path towards full container hermeticity.

  - .bin -- binary data; no trailing \\n is added.

Omitted file types:

  - `.tsv_map` or similar -- we could add an extension to allow a collection
    of options that will never need quoting to be presented as a single
    `.tsv_map` file.  This is not yet specified because there are some
    unresolved design questions, e.g. (i) whether this should support list of
    values, (ii) whether tabs should be permitted in values (i.e. only the
    first tab on a line is significant).

  - We don't have a special extension for ints or bools because (a) the
    automatic serialization/deserialization does not need to distinguish
    them from strings -- the consumer code do type coercion from string as
    needed, (b) it's generally obvious to the human what the data type
    should be.


## Design notes -- read before implementing or extending the format

Most low-level design decisions emerge from this short list of invariants:

Invariants:
  - Given an extension, and the serialized file contents, it has a unique
    deserialization to a JSON-style data structure.
  - Given an extension, and the deserialized data structure, it has a
    unique serialization to the extension's on-disk format.
  - Formats aside from `.bin` ought to be usable in coreutils-style scripts.
  - `serialize(deserialize_untyped(x)) == x` -- but the opposite is not
    guaranteed, e.g. all scalars will be stringified.

Convention: in Python identifiers, the . of the extension maps to __.
"""
import os
from typing import Any, List

from antlir.fs_utils import Path


# pyre-fixme[3]: Return type must be annotated.
def _make_script(dest: bytes, cmds: List[str]):
    return [
        # Write with `noclobber` to ensure that we fail if the file exists.
        "bash",
        "-ue",
        "-o",
        "noclobber",
        "-c",
        " ; ".join(
            [
                # Set umask to 0022 because bash's redirect mode is 0666, and we
                # want file permissiosn to be 0644.
                "umask 0022",
                # pyre-fixme[16]: `bytes` has no attribute `shell_quote`.
                "dest=" + dest.shell_quote(),
                'dest_dir=$(dirname "$dest")',
                # This won't make any directories outside the subvolume, since
                # `run_as_root` asserts that the subvolume exists.  The presumed
                # use-case is to make `/.meta/private/whatever/parent` inside a
                # subvolume, without the client code having to worry about it.
                # This auto-creation is OK since all metadata at present is
                # supposed to be 0755 root:root.
                'mkdir -p --mode=0755 "$dest_dir"',
            ]
            + cmds
        ),
    ]


# pyre-fixme[2]: Parameter annotation cannot be `Any`.
# pyre-fixme[2]: Parameter must be annotated.
def serialize(data: Any, subvol, path_with_ext: str) -> None:
    """
    Writes `data` to `path_with_ext` inside `subvol`.  The extension
    part of `path_with_ext` determines the serialization mechanism.

    Fails if the output file or directory exists.  Creates files with mode
    0644, directories with mode 0755.  Both get root:root ownership.
    """
    if data is None:
        return  # Write nothing, `None` corresponds to the "no such file".

    _, ext = os.path.splitext(path_with_ext)
    assert isinstance(ext, str)
    trailing_newline = b"\n"
    if ext in (".image_path", ".host_path"):
        if not isinstance(data, (str, bytes)):
            raise AssertionError(
                f"{path_with_ext} needs str/bytes, got {data} / {type(data)}"
            )
    elif ext == ".bin":
        trailing_newline = b""
        if not isinstance(data, bytes):
            raise AssertionError(
                f"{path_with_ext} needs bytes, got {data} / {type(data)}"
            )
    elif ext != "":
        raise AssertionError(f"Unsupported extension {path_with_ext}")

    if isinstance(data, dict):
        subvol.run_as_root(
            _make_script(
                subvol.path(path_with_ext),
                [
                    # Lacks -p since we want to fail if the dir exists
                    'mkdir --mode=0755 "$dest"'
                ],
            )
        )
        for k, v in data.items():
            serialize(v, subvol, os.path.join(path_with_ext, k))
        return

    if isinstance(data, bool):  # bool is a subclass of int, so check this first
        out_bytes = str(int(data)).encode()
    elif isinstance(data, (str, int, float)):
        out_bytes = str(data).encode()
    elif isinstance(data, bytes):
        out_bytes = data
    elif isinstance(data, list):
        raise AssertionError("add list support if you need it")
    else:
        raise AssertionError(f"unhandled type {type(data)} {data}")

    subvol.run_as_root(
        _make_script(subvol.path(path_with_ext), ['cat > "$dest"']),
        input=(out_bytes + trailing_newline),
    )


# pyre-fixme[3]: Return annotation cannot be `Any`.
def deserialize_untyped(path: Path, path_with_ext: str) -> Any:
    # `isdir` and `isfile` follow symbolic links so use `normalized_subpath`
    # to prevent the use of symlinks that take us outside the base path.
    if os.path.isdir(path.normalized_subpath(path_with_ext)):
        return {
            k: deserialize_untyped(path, os.path.join(path_with_ext, k))
            for k in os.listdir(path.normalized_subpath(path_with_ext).decode())
        }
    elif os.path.isfile(path.normalized_subpath(path_with_ext)):
        with open(path.normalized_subpath(path_with_ext), "rb") as f:
            s = f.read()

        _, ext = os.path.splitext(path_with_ext)
        if ext == ".bin":
            return s

        # All other extensions had a trailing newline appended.
        if not s.endswith(b"\n"):
            raise AssertionError(
                f"{path_with_ext} must have had a trailing newline, got {s}"
            )
        s = s[:-1]

        if ext == ".image_path" or ext == ".host_path":
            return s
        elif ext == "":
            return s.decode()
        else:
            raise AssertionError(f"Unsupported extension {path_with_ext}")
    else:
        raise AssertionError(f"{path_with_ext} is neither a file nor a dir")


# pyre-fixme[2]: Parameter must be annotated.
def deserialize_int(*args, **kwargs) -> int:
    return int(deserialize_untyped(*args, **kwargs))
