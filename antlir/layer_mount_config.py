#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"IMPORTANT: This is ONLY meant to be called from `_image_layer_impl`"
import json


def main(stdin, stdout, layer_target) -> None:
    stdin = stdin.read().strip()
    if stdin:
        mount_config = json.loads(stdin)
    else:
        mount_config = {}

    for key in ("build_source", "is_directory"):
        if key in mount_config:
            raise RuntimeError(
                f"`{key}` must not be set in `mount_config = {mount_config}`"
            )

    mount_config["is_directory"] = True
    mount_config["build_source"] = {
        # Don't attempt to target-tag this because this would complicate
        # MountItem, which would have to contain `Subvol` and know how to
        # serialize it (P106589820).  This is much messier than the current
        # approach of explicit target & layer lookups in `_BuildSource`.
        "source": layer_target,
        # The compiler knows how to resolve the above target to a layer path.
        # For now, we don't support mounting a subdirectory of a layer because
        # that might make packaging more complicated, but it could be done.
        "type": "layer",
    }

    json.dump(mount_config, stdout)


def invoke_main() -> None:  # pragma: no cover
    import sys

    main(sys.stdin, sys.stdout, *sys.argv[1:])


if __name__ == "__main__":
    invoke_main()  # pragma: no cover
