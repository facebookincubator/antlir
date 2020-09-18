#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from typing import Mapping, Optional, Sequence, Tuple

from .data import data


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
        self.assertEqual(c.appears_in, [4, 5, 6])
        self.assertEqual(
            [f.name for f in c.friends], ["Han Solo", "Leia Organa", "C-3PO"]
        )
        self.assertEqual(c.lightsaber_color, "green")
        self.assertEqual(c.callsign, ("Red", 5))
        self.assertEqual(c.metadata, {"species": "human"})

        # json_file and python_data produce identical objects
        self.assertEqual(c, characters[0])

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
            "appears_in": Sequence[int],
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
        self.maxDiff = None
        self.assertEqual(
            repr(characters[0]),
            "shape("
            "appears_in=[4, 5, 6], "
            "friends=[shape(name='Han Solo'), shape(name='Leia Organa'), shape(name='C-3PO')], "
            "name='Luke Skywalker', "
            "callsign=('Red', 5), "
            "lightsaber_color='green', "
            "metadata={'species': 'human'}"
            ")",
        )

    def test_class_repr(self):
        # The generated classes also have a custom repr, which is much more
        # readable
        self.maxDiff = None
        self.assertEqual(
            repr(character_t),
            "shape("
            "appears_in=Sequence[int], "
            "friends=Sequence[shape(name=str)], "
            "name=str, "
            "callsign=Optional[Tuple[str, int]], "
            "lightsaber_color=Optional[str], "
            "metadata=Mapping[str, str]"
            ")",
        )

    def test_immutable_fields(self):
        with self.assertRaises(TypeError):
            characters[0].name = "Darth Vader's son"
