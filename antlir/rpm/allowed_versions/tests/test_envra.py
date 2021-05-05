# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from functools import total_ordering
from unittest import TestCase

from antlir.rpm.rpm_metadata import RpmMetadata

from ..envra import SortableEVRA, SortableENVRA


class EnvraTestCase(TestCase):
    def test_evra_is_envra(self):
        self.assertIs(SortableEVRA, SortableENVRA)

    def test_eq(self):
        e = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        self.assertEqual(e, e)

    def test_lt(self):
        e0 = SortableENVRA(
            epoch=0, name="n", version="v", release="r", arch="a"
        )
        e1 = SortableENVRA(
            epoch=1, name="n", version="v", release="r", arch="a"
        )
        self.assertTrue(e0 < e1)

    def test_repr(self):
        e = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        self.assertEqual(str(e), "0:n-v-r-a")
        epoch_none = SortableENVRA(
            epoch=None, name="n", version="v", release="r", arch="a"
        )
        self.assertEqual(str(epoch_none), "*:n-v-r-a")
        name_none = SortableENVRA(
            epoch=0, name=None, version="v", release="r", arch="a"
        )
        self.assertEqual(str(name_none), "0:*-v-r-a")

    def test_to_versionlock_line_raise(self):
        epoch_none = SortableENVRA(
            epoch=None, name="n", version="v", release="r", arch="a"
        )
        with self.assertRaises(ValueError):
            epoch_none.to_versionlock_line()
        name_none = SortableENVRA(
            epoch=0, name=None, version="v", release="r", arch="a"
        )
        with self.assertRaises(ValueError):
            name_none.to_versionlock_line()

    def test_to_versionlock_line_returns(self):
        e = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        self.assertEqual(e.to_versionlock_line(), "0\tn\tv\tr\ta")

    def test_compare_returns_negative(self):
        e0 = SortableENVRA(
            epoch=0, name="m", version="v", release="r", arch="a"
        )
        e1 = SortableENVRA(
            epoch=0, name="n", version="v", release="r", arch="a"
        )
        self.assertTrue(e0 < e1)

    def test_compare_raise(self):
        @total_ordering
        class Crazy:
            def __eq__(self, other):
                return False

            def __lt__(self, other):
                return False

            def __gt__(self, other):
                return False

        e0 = SortableENVRA(
            epoch=0, name=Crazy(), version="v", release="r", arch="a"
        )
        e1 = SortableENVRA(
            epoch=0, name=Crazy(), version="v", release="r", arch="a"
        )
        with self.assertRaises(AssertionError):
            self.assertTrue(e0 < e1)

    def test_compare_both_epochs_wildcard(self):
        e = SortableENVRA(
            epoch=None, name="n", version="v", release="r", arch="a"
        )
        self.assertEqual(e, e)

    def test_compare_one_epoch_wildcard(self):
        e0 = SortableENVRA(
            epoch=None, name="n", version="v", release="r", arch="a"
        )
        e1 = SortableENVRA(
            epoch=0, name="n", version="v", release="r", arch="a"
        )
        with self.assertRaises(TypeError):
            self.assertEqual(e0, e1)

    def test_compare_self_greater_than_other(self):
        e0 = SortableENVRA(
            epoch=0, name="n", version="v", release="r", arch="a"
        )
        e1 = SortableENVRA(
            epoch=0, name="m", version="v", release="r", arch="a"
        )
        self.assertFalse(e0 < e1)

    def test_compare_both_names_wildcard(self):
        e = SortableENVRA(
            epoch=0, name=None, version="v", release="r", arch="a"
        )
        self.assertEqual(e, e)

    def test_compare_one_name_wildcard(self):
        e0 = SortableENVRA(
            epoch=0, name=None, version="v", release="r", arch="a"
        )
        e1 = SortableENVRA(
            epoch=0, name="n", version="v", release="r", arch="a"
        )
        with self.assertRaises(TypeError):
            self.assertEqual(e0, e1)

    def test_as_rpm_metadata_returns(self):
        e = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        rpm_metadata = RpmMetadata(name="n", epoch=0, version="v", release="r")
        self.assertEqual(e.as_rpm_metadata(), rpm_metadata)

    def test_as_rpm_metadata_raise(self):
        e = SortableENVRA(
            epoch=None, name="n", version="v", release="r", arch="a"
        )
        with self.assertRaises(TypeError):
            e.as_rpm_metadata()
