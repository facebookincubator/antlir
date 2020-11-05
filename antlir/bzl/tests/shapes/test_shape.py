#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from typing import Mapping, Optional, Sequence, Tuple

from antlir.fs_utils import Path
from antlir.shape import Shape, Target

# TODO remove all references to hashable and just use characters once
# read-only dicts land
from .hashable_t import hashable_t
from .loader import shape
from .luke import data as luke


character_t = shape.types.characters
lightsaber_t = character_t.types.lightsaber
characters = shape.read_resource(__package__, "characters.json").characters


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
        # The target's on disk path is specific to the host that the test runs
        # on. So we need a static value that we can reliably test.  Since
        # models are immutable, we'll make a deep copy and udpate with a static
        # value.
        lightsaber_target_fixed = c.lightsaber.target.copy(
            update={"path": b"/static/target/path"}
        )
        lightsaber_fixed = c.lightsaber.copy(
            update={
                "target": lightsaber_target_fixed,
            },
        )
        self.assertEqual(
            lightsaber_fixed,
            lightsaber_t(
                color="green",
                target=Target(
                    name=":luke-lightsaber",
                    path=b"/static/target/path",
                ),
                layer_target=c.lightsaber.layer_target,
            ),
        )
        self.assertEqual(c.callsign, ("Red", 5))
        self.assertEqual(c.metadata, {"species": "human"})
        self.assertEqual(
            c.personnel_file, Path("/rebellion/luke_skywalker.txt")
        )
        self.assertIsInstance(c.personnel_file, Path)

        # json_file and python_data produce identical objects
        self.assertEqual(luke, characters[0])

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
            "callsign": Optional[Tuple[str, int]],
            "metadata": Mapping[str, str],
        }
        self.assertEqual(
            expected, {k: v for k, v in hints.items() if k in expected}
        )

    def test_instance_repr(self):
        # The on-disk path for resolved targets will be different between
        # environments, so we assign a static value so that we can compare
        # the repr properly.
        lightsaber_target_fixed = characters[0].lightsaber.target.copy(
            update={"path": b"/static/target/path"}
        )
        lightsaber_layer_fixed = characters[0].lightsaber.layer_target.copy(
            update={
                "path": b"/static/layer/path",
                "subvol": None,
            }
        )
        lightsaber_fixed = characters[0].lightsaber.copy(
            deep=True,
            update={
                "target": lightsaber_target_fixed,
                "layer_target": lightsaber_layer_fixed,
            },
        )

        # Shapes don't have nice classnames, so the repr is customized to be
        # human-readable. While this has no functional impact on the code, it
        # is critical for usability, so ensure there are unit tests.
        self.assertEqual(
            repr(characters[0].copy(update={"lightsaber": lightsaber_fixed})),
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
            "lightsaber=shape("
            + (
                "color='green', "
                "target=Target("
                "name=':luke-lightsaber', path=b'/static/target/path'"
                "), "
                "layer_target=LayerTarget("
                "name=':luke-lightsaber-layer', "
                "path=b'/static/layer/path', "
                "subvol=None"
                ")"
            )
            + "), "
            "callsign=('Red', 5), "
            "metadata={'species': 'human'}, "
            "affiliations=shape(faction='Rebellion'), "
            "personnel_file=b'/rebellion/luke_skywalker.txt'"
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
            "lightsaber=Optional["
            + (
                "shape("
                "color=str, "
                "target=Optional[Target], "
                "layer_target=Optional[LayerTarget]"
                ")"
            )
            + "], "
            "callsign=Optional[Tuple[str, int]], "
            "metadata=Mapping[str, str], "
            "affiliations=shape(faction=str), "
            "personnel_file=Optional[Path]"
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
            lightsaber=character_t.types.lightsaber(color="blue"),
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
            "lightsaber=shape(color='blue', target=None, layer_target=None), "
            "callsign=None, "
            "metadata={'species': 'human'}, "
            "affiliations=shape(faction='Jedi Temple'), "
            "personnel_file=None, "
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
            "lightsaber=Optional[shape("
            + (
                "color=str, "
                "target=Optional[Target], "
                "layer_target=Optional[LayerTarget]"
            )
            + ")], "
            "callsign=Optional[Tuple[str, int]], "
            "metadata=Mapping[str, str], "
            "affiliations=shape(faction=str), "
            "personnel_file=Optional[Path], "
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
