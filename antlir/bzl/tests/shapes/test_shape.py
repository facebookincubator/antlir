#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from typing import Mapping, Optional, Sequence, Tuple

from antlir.shape import Shape

from .data import data

# TODO remove all references to hashable and just use characters once
# read-only dicts land
from .hashable_t import hashable_t


character_t = data.__annotations__["characters"].__args__[0]
characters = data.characters


class TestShape(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def test_load(self):
        """load happy path from json_file, and python_data"""
        c = character_t.read_resource(__package__, "luke.json")

        self.assertEqual(c.name, "Luke Skywalker")
        self.assertEqual(c.appears_in, (4, 5, 6))
        self.assertEqual(
            [f.name for f in c.friends], ["Han Solo", "Leia Organa", "C-3PO"]
        )
        self.assertEqual(c.lightsaber_color, "green")
        self.assertEqual(c.callsign, ("Red", 5))
        self.assertEqual(c.metadata, {"species": "human"})

        # json_file and python_data produce identical objects
        self.assertEqual(c, characters[0])

    def test_hash(self):
        trooper1 = hashable_t(
            name="Stormtrooper",
            appears_in=[1, 2, 3, 4, 5, 6],
            friends=[],
            metadata=None,
        )
        trooper2 = hashable_t(
            name="Stormtrooper",
            appears_in=[1, 2, 3, 4, 5, 6],
            friends=[],
            metadata=None,
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
            "lightsaber_color": Optional[str],
            "callsign": Optional[Tuple[str, int]],
            "metadata": Mapping[str, str],
        }
        self.assertEqual(
            expected, {k: v for k, v in hints.items() if k in expected}
        )

    def test_instance_repr(self):
        # Shapes don't have nice classnames, so the repr is customized to be
        # human-readable. While this has no functional impact on the code, it
        # is critical for usability, so ensure there are unit tests.
        self.assertEqual(
            repr(characters[0]),
            "shape("
            "name='Luke Skywalker', "
            "appears_in=(4, 5, 6), "
            "friends=("
            + (
                "shape(name='Han Solo'), "
                "shape(name='Leia Organa'), "
                "shape(name='C-3PO')"
            )
            + "), "
            "lightsaber_color='green', "
            "callsign=('Red', 5), "
            "metadata={'species': 'human'}, "
            "affiliations=shape(faction='Rebellion')"
            ")",
        )

    def test_class_repr(self):
        # The generated classes also have a custom repr, which is much more
        # readable
        self.assertEqual(
            repr(character_t),
            "shape("
            "name=str, "
            "appears_in=Tuple[int, ...], "
            "friends=Tuple[shape(name=str), ...], "
            "lightsaber_color=Optional[str], "
            "callsign=Optional[Tuple[str, int]], "
            "metadata=Mapping[str, str], "
            "affiliations=shape(faction=str)"
            ")",
        )

    def test_immutable_fields(self):
        with self.assertRaises(TypeError):
            characters[0].name = "Darth Vader's son"

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
            lightsaber_color="blue",
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
            "name='Obi-Wan Kenobi', "
            "appears_in=(1, 2, 3, 4, 5, 6), "
            "friends=(shape(name='Yoda'), shape(name='Padme Amidala')), "
            "lightsaber_color='blue', "
            "callsign=None, "
            "metadata={'species': 'human'}, "
            "affiliations=shape(faction='Jedi Temple'), "
            "padawan='Anakin Skywalker'"
            ")",
        )
        # subclass type repr should also use the human-written class name
        self.assertEqual(
            repr(Jedi),
            "Jedi("
            "name=str, "
            "appears_in=Tuple[int, ...], "
            "friends=Tuple[shape(name=str), ...], "
            "lightsaber_color=Optional[str], "
            "callsign=Optional[Tuple[str, int]], "
            "metadata=Mapping[str, str], "
            "affiliations=shape(faction=str), "
            "padawan=Optional[str]"
            ")",
        )

        self.assertEqual(obi_wan.train(), "Training Anakin Skywalker")

    def test_nested_shape_class(self):
        self.assertEqual(
            character_t.types.affiliations(faction="shape.bzl").faction,
            "shape.bzl",
        )
        # collection fields should do the sane thing
        # dicts go to value type
        self.assertEqual(character_t.types.metadata, str)
        # tuples go to a tuple of element types
        self.assertEqual(character_t.types.callsign, (str, int))
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
