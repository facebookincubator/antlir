# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

affiliations_t = shape.shape(
    __typename__ = "Affiliations",
    faction = str,
)

# A shape that references buck targets
lightsaber_t = shape.shape(
    __typename__ = "Lightsaber",
    color = shape.enum(
        "red",
        "green",
        "blue",
        __typename__ = "Color",
    ),
    target = shape.target(optional = True),
)

# Test data adapted from the GraphQL Star Wars examples
character_t = shape.shape(
    __typename__ = "Character",
    name = str,
    appears_in = shape.list(int),
    friends = shape.list(shape.shape(
        __typename__ = "Friend",
        name = str,
    )),
    weapon = shape.union(
        lightsaber_t,
        str,
        optional = True,
    ),
    callsign = shape.tuple(
        str,
        int,
        optional = True,
    ),
    metadata = shape.dict(
        str,
        str,
        default = {"species": "human"},
    ),
    affiliations = shape.field(
        affiliations_t,
        default = shape.new(
            affiliations_t,
            faction = "Rebellion",
        ),
    ),
    personnel_file = shape.path(optional = True),
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
    callsign = shape.tuple(
        str,
        int,
        optional = True,
    ),
    metadata = shape.dict(
        str,
        str,
        default = {"species": "human"},
    ),
)
