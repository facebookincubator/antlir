# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

affiliations_t = shape.shape(
    faction = str,
    __thrift = {
        0: "faction",
    },
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
    __thrift = {
        0: "color",
        1: "target",
    },
)

weapon_t = shape.union(lightsaber_t, str, __thrift = [0, 1])
metadata_t = shape.dict(str, str)
friend_t = shape.shape(name = str, __thrift = {0: "name"})

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
        default = affiliations_t(
            faction = "Rebellion",
        ),
    ),
    personnel_file = shape.field(shape.path, True),
    __thrift = {
        0: "name",
        1: "appears_in",
        2: "friends",
        3: "weapon",
        4: "metadata",
        5: "affiliations",
        6: "personnel_file",
    },
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

with_optional_int = shape.shape(
    optint = shape.field(int, optional = True),
)

# Simulate two versions of a thrift struct, a new one with one new optional
# field, and one removed. The serialization should be mutually compatible
# (obviously with removed/added fields being null).
# As of this writing, the only compatible change is adding a new field. Later
# diffs in this stack will make it possible to remove or reorder fields.
thrift_old = shape.shape(
    foo = int,
    # before remove, a deprecated field must be made optional
    bar = shape.field(str, optional = True),
    __thrift = {
        0: "foo",
        1: "bar",
    },
)

thrift_new = shape.shape(
    foo = int,
    # before full rollout, a new field must be optional
    baz = shape.field(str, optional = True, default = "baz"),
    qux = shape.field(bool, optional = True),
    __thrift = {
        0: "foo",
        2: "baz",
        3: "qux",
    },
)

union_old = shape.union(str, int, __thrift = [0, 1])
union_new = shape.union(int, bool, __thrift = [1, 2])

inner = shape.shape(
    a = shape.field(str, optional = True, default = "def"),
    __thrift = {
        0: "a",
    },
)

with_default_trait = shape.shape(
    a = shape.field(str, optional = True, default = "abc"),
    b = shape.field(bool, default = True),
    c = shape.field(inner, default = inner()),
    __thrift = {
        0: "a",
        1: "b",
        2: "c",
    },
)
