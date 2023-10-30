#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import json
import pickle
import tempfile
import unittest

from antlir.buck.targets_and_outputs.targets_and_outputs_py import TargetsAndOutputs
from antlir.fs_utils import Path


class Test(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_fails_on_unqualified_labels(self) -> None:
        """
        Fail on unqualified target labels.
        The serialization process ensures that the default cell is always added,
        only serializing fully-qualified labels to the json file.
        """
        with self.assertRaisesRegex(
            Exception, ".*label '//in/foo:unqual' does not match the regex.*"
        ):
            TargetsAndOutputs.from_json_str(
                json.dumps(
                    {
                        "metadata": {
                            "buck_version": 2,
                            "default_cell": "foo",
                        },
                        "targets_and_outputs": {
                            "//in/foo:unqual": "/path/to/unqual",
                            "other//path/to:other": "/path/to/other",
                        },
                    }
                )
            )

    def test_unqualified_get(self) -> None:
        """
        Unqualified labels should immediately fail
        """
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            (tmpdir / ".buckconfig").touch()
            tao = TargetsAndOutputs.from_json_str(
                json.dumps(
                    {
                        "metadata": {
                            "buck_version": 2,
                            "default_cell": "foo",
                        },
                        "targets_and_outputs": {
                            "foo//path/to:target": "etc/hostname",
                        },
                    }
                ),
                path_in_repo=tmpdir,
            )
            with self.assertRaisesRegex(ValueError, "does not match the regex"):
                tao["//path/to:target"]

    def test_absolutizes_paths(self) -> None:
        """
        Always returns absolute paths to Python.
        Returns absolute paths to Python, even when serialization is relative to
        the buck root (cwd where the serializer binary ran from)
        """
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            (tmpdir / ".buckconfig").touch()
            tao = TargetsAndOutputs.from_json_str(
                json.dumps(
                    {
                        "metadata": {
                            "buck_version": 2,
                            "default_cell": "foo",
                        },
                        "targets_and_outputs": {
                            "foo//path/to:target": "etc/hostname",
                        },
                    }
                ),
                path_in_repo=tmpdir,
            )
            self.assertIsInstance(tao["foo//path/to:target"], Path)
            self.assertEqual(tmpdir / "etc/hostname", tao["foo//path/to:target"])

    def test_missing(self) -> None:
        """
        Missing behavior is correct.
        Returns `None` when `get()` is called, or raises a KeyError when called
        via subscript
        """
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            (tmpdir / ".buckconfig").touch()
            tao = TargetsAndOutputs.from_json_str(
                json.dumps(
                    {
                        "metadata": {
                            "buck_version": 2,
                            "default_cell": "foo",
                        },
                        "targets_and_outputs": {},
                    }
                ),
                path_in_repo=tmpdir,
            )
            self.assertIsNone(tao.get("foo//path/to:target"))
            with self.assertRaises(KeyError):
                tao["foo//path/to:target"]

    def test_pickle(self) -> None:
        """
        Pickle/unpickle works.
        Some of antlir pickles compiler args as a whole blob, so
        TargetsAndOutputs must support it!
        """
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            (tmpdir / ".buckconfig").touch()
            tao = TargetsAndOutputs.from_json_str(
                json.dumps(
                    {
                        "metadata": {
                            "buck_version": 2,
                            "default_cell": "foo",
                        },
                        "targets_and_outputs": {
                            "foo//path/to:target": "etc/hostname",
                        },
                    }
                ),
                path_in_repo=tmpdir,
            )
            # round-trip it through pickle
            pickled = pickle.dumps(tao)
            tao = pickle.loads(pickled)
            self.assertIsInstance(tao, TargetsAndOutputs)
            self.assertEqual(tmpdir / "etc/hostname", tao["foo//path/to:target"])

            # do it one more time but as part of a dictionary
            pickled = pickle.dumps({"tao": tao})
            tao = pickle.loads(pickled)["tao"]
            self.assertIsInstance(tao, TargetsAndOutputs)
            self.assertEqual(tmpdir / "etc/hostname", tao["foo//path/to:target"])

    def _test_object_new(self) -> None:
        """
        object.__new__(TargetsAndOutputs) works
        Pickle does this (sometimes?) so it should pass and create an empty
        TargetsAndOutputs
        """
        tao = object.__new__(TargetsAndOutputs)
        self.assertIsInstance(tao, TargetsAndOutputs)
        self.assertEqual(0, len(tao))

    def test_repr(self) -> None:
        """
        Repr should show a readable representation instead of the useless
        default pointer address.
        """
        with tempfile.TemporaryDirectory() as tmpdir:
            tmpdir = Path(tmpdir)
            (tmpdir / ".buckconfig").touch()
            tao = TargetsAndOutputs.from_json_str(
                json.dumps(
                    {
                        "metadata": {
                            "buck_version": 2,
                            "default_cell": "foo",
                        },
                        "targets_and_outputs": {
                            "foo//path/to:target": "etc/hostname",
                        },
                    }
                ),
                path_in_repo=tmpdir,
            )
            self.assertEqual(repr(tao), "{'foo//path/to:target': 'etc/hostname'}")
