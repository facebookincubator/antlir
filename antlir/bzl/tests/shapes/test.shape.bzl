# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

affiliations_t = shape.shape(
    faction = str,
)

color_t = shape.enum(
    "red",
    "green",
    "blue",
)

# A shape that references buck targets
lightsaber_t = shape.shape(
    color = color_t,
    target = shape.field(target_t, optional = True),
)

weapon_t = shape.union(lightsaber_t, str)
metadata_t = shape.dict(str, str)
friend_t = shape.shape(name = str)

# Test data adapted from the GraphQL Star Wars examples
character_t = shape.shape(
    name = str,
    appears_in = shape.list(int),
    friends = shape.list(friend_t),
    weapon = shape.field(
        weapon_t,
        optional = True,
    ),
    metadata = shape.field(
        metadata_t,
        default = {"species": "human"},
    ),
    affiliations = shape.field(
        affiliations_t,
        default = shape.new(
            affiliations_t,
            faction = "Rebellion",
        ),
    ),
    personnel_file = shape.field(shape.path, True),
)

character_collection_t = shape.shape(characters = shape.list(character_t))

# TODO when read-only dicts land just use character_t
hashable_t = shape.shape(
    name = str,
    appears_in = shape.list(int),
    lightsaber_color = shape.field(
        str,
        optional = True,
    ),
    metadata = shape.field(
        metadata_t,
        default = {"species": "human"},
    ),
)
