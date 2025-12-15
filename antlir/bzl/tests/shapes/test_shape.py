#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib
import unittest
from typing import Mapping, Optional, Sequence, Tuple

from antlir.bzl.target import target_t

# TODO remove all references to hashable and just use characters once
# read-only dicts land
from antlir.bzl.tests.shapes.test import (
    character_collection_t,
    character_t,
    friend_t,
    hashable_t,
)

from antlir.freeze import frozendict
from antlir.fs_utils import Path
from antlir.shape import Shape


# pyre-fixme[16]: `character_t` has no attribute `types`.
lightsaber_t = character_t.types.weapon.__args__[0]
characters = character_collection_t.from_env("characters").characters

# @oss-disable[end= ]: TARGET_PATH = "fbcode//antlir/bzl/tests/shapes"
TARGET_PATH = "antlir//antlir/bzl/tests/shapes" # @oss-enable


class TestShape(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def test_load(self):
        c = characters[0]
        self.assertEqual(c.name, "Luke Skywalker")
        self.assertEqual(c.appears_in, (4, 5, 6))
        self.assertEqual(
            [f.name for f in c.friends], ["Han Solo", "Leia Organa", "C-3PO"]
        )
        self.assertEqual(
            c.weapon,
            lightsaber_t(
                color=lightsaber_t.types.color.GREEN,
                target=target_t(
                    name=f"{TARGET_PATH}:luke-lightsaber",
                    path=b"",
                ),
            ),
        )
        self.assertEqual(c.metadata, frozendict({"species": "human"}))
        self.assertEqual(c.personnel_file, Path("/rebellion/luke_skywalker.txt"))
        self.assertIsInstance(c.personnel_file, Path)

    def test_data_and_resources(self):
        # hashable_t is just a (incomplete) subset of the character type that
        # is both hashable and serializable to python_data/json_file
        res = hashable_t.read_resource(__package__, "data.json")
        self.assertEqual(res.name, "Stormtrooper")
        # load the same thing from a file path
        with importlib.resources.path(__package__, "data.json") as path:
            f = hashable_t.load(path)
        self.assertEqual(f, res)
        # lastly, the directly imported python_data version should also be
        # equivalent
        from antlir.bzl.tests.shapes.data import data as imp

        self.assertEqual(imp, res)
        self.assertTrue(isinstance(imp, hashable_t))

    def test_json_file_targets(self):
        with importlib.resources.path(__package__, "characters.json") as path:
            c = character_collection_t.load(path)
        target = c.characters[0].weapon.target
        self.assertEqual(target.name, f"{TARGET_PATH}:luke-lightsaber")

    def test_hash(self):
        trooper1 = hashable_t(
            name="Stormtrooper",
            appears_in=[1, 2, 3, 4, 5, 6],
            friends=[],
        )
        trooper2 = hashable_t(
            name="Stormtrooper",
            appears_in=[1, 2, 3, 4, 5, 6],
            friends=[],
        )
        self.assertEqual(trooper1.__hash__(), trooper2.__hash__())

    def test_typehints(self):
        """check type hints on generated classes"""

        def _deep_typehints(obj):
            hints = {}
            for key, val in getattr(obj, "__annotations__", {}).items():
                if hasattr(val, "__annotations__"):
                    hints[key] = _deep_typehints(val)
                else:
                    hints[key] = val
            return hints

        # non-nested type hints can easily be compared
        hints = _deep_typehints(character_t)
        expected = {
            "name": str,
            "appears_in": Tuple[int, ...],
            "metadata": Mapping[str, str],
        }
        self.assertEqual(expected, {k: v for k, v in hints.items() if k in expected})

    def test_instance_repr(self):
        # Shapes don't always have nice classnames, so the repr is customized to
        # be human-readable. While this has no functional impact on the code, it
        # is critical for usability, so ensure there are unit tests.
        self.assertEqual(
            repr(characters[0]),
            "shape("
            "affiliations=shape(faction='Rebellion'), "
            "appears_in=(4, 5, 6), "
            "friends=("
            + (
                "shape(name='Han Solo'), "
                "shape(name='Leia Organa'), "
                "shape(name='C-3PO')"
            )
            + "), "
            "metadata=frozendict({'species': 'human'}), "
            "name='Luke Skywalker', "
            "personnel_file=b'/rebellion/luke_skywalker.txt', "
            "weapon=shape("
            + (
                "color=GREEN, "
                "target=shape("
                f"name='{TARGET_PATH}:luke-lightsaber', "
                "path=b''"
                ")"
            )
            + "))",
        )

    def test_class_repr(self):
        # The generated classes also have a custom repr, which is much more
        # readable. However, it's a huge pain to test that it actually looks
        # good, so just make sure it doesn't look like the default python class
        # repr
        self.assertNotIn("<class", repr(character_t))
        self.assertTrue(repr(character_t).startswith("shape("))

    def test_immutable_fields(self):
        with self.assertRaises(TypeError):
            characters[0].name = "Darth Vader's son"
        # immutability is a property at any level of nesting
        with self.assertRaises(TypeError):
            characters[0].friends[0].name = "R2-D2"
        # shouldn't be able to add to any lists
        new_friend = friend_t(name="R2-D2")
        with self.assertRaises(AttributeError):
            characters[0].friends.append(new_friend)
        # or dictionaries
        with self.assertRaises(TypeError):
            characters[0].metadata["favorite-food"] = "Blue Milk"

    def test_subclass(self):
        """
        Demonstrate a pure Python subclass of a shape type with added fields
        and functions.
        """

        class Jedi(character_t):
            padawan: Optional[str]

            def train(self) -> str:
                if self.padawan:
                    return f"Training {self.padawan}"
                return "No one to train right now"

        obi_wan = Jedi(
            padawan="Anakin Skywalker",
            name="Obi-Wan Kenobi",
            appears_in=[1, 2, 3, 4, 5, 6],
            # TODO: there is not a good way to get at the type of an inner
            # shape definition. It's not super important at this point since
            # they should really only be deserialized from JSON that comes from
            # Buck macros, and in the rare case that it's necessary, a dict
            # with the same fields works and is properly validated.
            friends=[{"name": "Yoda"}, {"name": "Padme Amidala"}],
            weapon=lightsaber_t(color="blue"),
            affiliations=character_t.types.affiliations(faction="Jedi Temple"),
        )
        # subclass should still be immutable by default
        with self.assertRaises(TypeError):
            obi_wan.padawan = "Luke Skywalker"

        # subclass instance repr uses the human-written class name, and the
        # inner shapes use shape()
        self.assertEqual(
            repr(obi_wan),
            "Jedi("
            "affiliations=shape(faction='Jedi Temple'), "
            "appears_in=(1, 2, 3, 4, 5, 6), "
            "friends=(shape(name='Yoda'), shape(name='Padme Amidala')), "
            "metadata=frozendict({'species': 'human'}), "
            "name='Obi-Wan Kenobi', "
            "personnel_file=None, "
            "weapon=shape(color=BLUE, target=None), "
            "padawan='Anakin Skywalker'"
            ")",
        )
        # subclass type repr should also use the human-written class name
        self.assertTrue(repr(Jedi).startswith("Jedi("))
        # added field shows up
        self.assertIn("padawan=", repr(Jedi))

        self.assertEqual(obi_wan.train(), "Training Anakin Skywalker")

    def test_nested_shape_class(self):
        self.assertEqual(
            character_t.types.affiliations(faction="shape.bzl").faction,
            "shape.bzl",
        )
        # collection fields should do the sane thing
        # dicts go to value type
        self.assertEqual(character_t.types.metadata, str)
        # lists go to the homogenous element type
        self.assertEqual(
            character_t.types.friends,
            character_t.__annotations__["friends"].__args__[0],
        )

    def test_default_shape(self):
        """default values for nested shapes must be deserialized"""
        c3po = characters[2]
        self.assertEqual(c3po.name, "C-3PO")
        self.assertEqual(c3po.affiliations.faction, "Rebellion")

    def test_failure_when_has_types_field(self):
        with self.assertRaises(KeyError):

            class AnnotationOnly(Shape):
                # fake that this came from a class defined by shape.bzl
                __GENERATED_SHAPE__ = True
                # this is reserved for the type definitions class
                types: Sequence[str]

        with self.assertRaises(KeyError):

            class WithDefault(Shape):
                # fake that this came from a class defined by shape.bzl
                __GENERATED_SHAPE__ = True
                # this is reserved for the type definitions class
                types: Sequence[str] = ("hello", "world")

    def test_rendered_template(self):
        self.assertEqual(
            "Stormtrooper is a character that appears in episode(s) 1, 2, 3, 4, 5 and 6 of\n"
            "the Star Wars franchise and is friends with Vader, Palpatine and Tarkin.",
            importlib.resources.read_text(__package__, "template.txt"),
        )
