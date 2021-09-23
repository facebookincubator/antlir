#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


# Until there is a proper container runtime for vmtest for sidecars like this,
# we have to do this hacky workaround in order to run `radvd` from an upstream
# rpm - we run the radvd installed into a container, but just bare on the host.
# This only works because radvd does not have any other dependencies that exist
# only in the container.

from antlir.tests.layer_resource import layer_resource_subvol


def start_router_advertisements():
    radvd_layer = layer_resource_subvol(__package__, "radvd-layer")
    with radvd_layer.popen_as_root(
        [
            radvd_layer.path("/usr/sbin/radvd"),
            "--nodaemon",
            "--config",
            radvd_layer.path("/etc/radvd.conf"),
        ]
    ) as proc:
        proc.wait()


if __name__ == "__main__":
    start_router_advertisements()
