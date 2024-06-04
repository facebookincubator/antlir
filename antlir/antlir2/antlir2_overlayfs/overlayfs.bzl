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
    json_file = typing.Any,
)
