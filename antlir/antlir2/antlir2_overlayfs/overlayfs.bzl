# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# See src/model.rs for what all these actually mean

OverlayLayer = record(
    data_dir = Artifact | OutputArtifact,
    manifest = Artifact | OutputArtifact,
)

OverlayFs = record(
    top = OverlayLayer,
    # left-to-right lowest-to-highest
    layers = field(list[OverlayLayer], default = []),
    # {top, layers} serialized as a json file (with_inputs = True)
    json_file_with_inputs = typing.Any,
    # Same file as above, but without associated inputs
    json_file = Artifact | None,
)

_KEY = "antlir2.overlayfs"

def set_antlir2_use_overlayfs():
    write_package_value(
        _KEY,
        True,
    )

def get_antlir2_use_overlayfs() -> bool:
    return read_package_value(_KEY) or False
