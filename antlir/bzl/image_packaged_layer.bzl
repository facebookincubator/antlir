# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:partial.bzl", "partial")
load("//antlir/bzl:image_package.bzl", "image_package")

def image_packaged_layer(
        layer_name,
        publisher_name,
        *,
        partial_layer,
        partial_publisher):
    """
`image.packaged_layer` is a small wrapper around `image.layer` to support both
creating a layer and including a reference to a corresponding 'publisher' target
within that layer, which is then responsible for publishing that layer as a
squashfs package to an external artifact store.

Args:

    layer_name: Target name that will be given to `partial_layer`.

    publisher_name: Target name that will be given to `partial_publisher`.

    partial_layer: A partial `image.layer` object that will be supplied with a
        custom `mount_config` and located under `name`.

    partial_publisher: A partial target supporting a `path_actions` argument,
        which will be provided by the implementation. When run, this target
        should publish the targets in `path_actions` to an artifact store.
    """
    img_pkg_name = "{}=layer.sqfs".format(layer_name)
    image_package(
        name = img_pkg_name,
        layer = ":" + layer_name,
        format = "squashfs",
    )
    partial.call(
        partial_publisher,
        name = publisher_name,
        path_actions = {"image.sqfs": ":" + img_pkg_name},
    )
    partial.call(
        partial_layer,
        name = layer_name,
        mount_config = {
            "default_mountpoint": "/packages/" + publisher_name,
            "layer_publisher": {
                "package": publisher_name,
                # Depend on the publisher target from the layer to be able to
                # query the publisher from a given layer target. Note that this
                # means this publisher target cannot depend on the layer in its
                # `path_actions` to avoid a circular dependency.
                "target_path": "$(query_targets :{})".format(publisher_name),
            },
        },
    )
