# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from functools import total_ordering
from unittest import TestCase

from antlir.rpm.allowed_versions.envra import SortableENVRA, SortableEVRA

from antlir.rpm.rpm_metadata import RpmMetadata


class EnvraTestCase(TestCase):
    def test_evra_is_envra(self) -> None:
        self.assertIs(SortableEVRA, SortableENVRA)

    def test_eq(self) -> None:
        e = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        self.assertEqual(e, e)

    def test_lt(self) -> None:
        e0 = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        e1 = SortableENVRA(epoch=1, name="n", version="v", release="r", arch="a")
        self.assertTrue(e0 < e1)

    def test_repr(self) -> None:
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
        arch_none = SortableENVRA(
            epoch=0,
            name="n",
            version="v",
            release="r",
            # pyre-fixme[6]: For 5th param expected `str` but got `None`.
            arch=None,
        )
        self.assertEqual(str(arch_none), "0:n-v-r-*")

    def test_to_versionlock_line_raise(self) -> None:
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
        arch_none = SortableENVRA(
            epoch=0,
            name="n",
            version="v",
            release="r",
            # pyre-fixme[6]: For 5th param expected `str` but got `None`.
            arch=None,
        )
        with self.assertRaises(ValueError):
            arch_none.to_versionlock_line()

    def test_to_versionlock_line_returns(self) -> None:
        e = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        self.assertEqual(e.to_versionlock_line(), "0\tn\tv\tr\ta")

    def test_compare_returns_negative(self) -> None:
        e0 = SortableENVRA(epoch=0, name="m", version="v", release="r", arch="a")
        e1 = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        self.assertTrue(e0 < e1)

    def test_compare_raise(self) -> None:
        @total_ordering
        class Crazy:
            def __eq__(self, other):
                return False

            def __lt__(self, other):
                return False

            def __gt__(self, other):
                return False

        e0 = SortableENVRA(
            epoch=0,
            # pyre-fixme[6]: For 2nd param expected `Optional[str]` but got `Crazy`.
            name=Crazy(),
            version="v",
            release="r",
            arch="a",
        )
        e1 = SortableENVRA(
            epoch=0,
            # pyre-fixme[6]: For 2nd param expected `Optional[str]` but got `Crazy`.
            name=Crazy(),
            version="v",
            release="r",
            arch="a",
        )
        with self.assertRaises(AssertionError):
            self.assertTrue(e0 < e1)

    def test_compare_both_epochs_wildcard(self) -> None:
        e = SortableENVRA(epoch=None, name="n", version="v", release="r", arch="a")
        self.assertEqual(e, e)

    def test_compare_one_epoch_wildcard(self) -> None:
        e0 = SortableENVRA(epoch=None, name="n", version="v", release="r", arch="a")
        e1 = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        with self.assertRaises(TypeError):
            self.assertEqual(e0, e1)

    def test_compare_both_archs_wildcard(self) -> None:
        e = SortableENVRA(
            epoch=0,
            name="n",
            version="v",
            release="r",
            # pyre-fixme[6]: For 5th param expected `str` but got `None`.
            arch=None,
        )
        self.assertEqual(e, e)

    def test_compare_one_arch_wildcard(self) -> None:
        e0 = SortableENVRA(
            epoch=0,
            name="n",
            version="v",
            release="r",
            # pyre-fixme[6]: For 5th param expected `str` but got `None`.
            arch=None,
        )
        e1 = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        with self.assertRaises(TypeError):
            self.assertEqual(e0, e1)

    def test_compare_self_greater_than_other(self) -> None:
        e0 = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        e1 = SortableENVRA(epoch=0, name="m", version="v", release="r", arch="a")
        self.assertFalse(e0 < e1)

    def test_compare_both_names_wildcard(self) -> None:
        e = SortableENVRA(epoch=0, name=None, version="v", release="r", arch="a")
        self.assertEqual(e, e)

    def test_compare_one_name_wildcard(self) -> None:
        e0 = SortableENVRA(epoch=0, name=None, version="v", release="r", arch="a")
        e1 = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        with self.assertRaises(TypeError):
            self.assertEqual(e0, e1)

    def test_as_rpm_metadata_returns(self) -> None:
        e = SortableENVRA(epoch=0, name="n", version="v", release="r", arch="a")
        rpm_metadata = RpmMetadata(name="n", epoch=0, version="v", release="r")
        self.assertEqual(e.as_rpm_metadata(), rpm_metadata)

    def test_as_rpm_metadata_raise(self) -> None:
        epoch_none = SortableENVRA(
            epoch=None, name="n", version="v", release="r", arch="a"
        )
        with self.assertRaises(TypeError):
            epoch_none.as_rpm_metadata()
        arch_none = SortableENVRA(
            epoch=0,
            name="n",
            version="v",
            release="r",
            # pyre-fixme[6]: For 5th param expected `str` but got `None`.
            arch=None,
        )
        with self.assertRaises(TypeError):
            arch_none.as_rpm_metadata()
