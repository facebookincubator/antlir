#!/usr/bin/env python3
import unittest
from random import randint
from typing import Iterable, List, Mapping, Set, Tuple

from ..common import Checksum
from ..repo_objects import Rpm, Repodata
from ..repo_sizer import RepoSizer

_FAKE_RPM = Rpm(*([None] * len(Rpm._fields)))


class RepoSizerTestCase(unittest.TestCase):
    def setUp(self):
        self.sizes = {}
        self.final_size = 0

    def _set_expected_unions(self, *expected_unions: Iterable[Set[str]]):
        for union in expected_unions:
            size = randint(10**3, 10**6)
            self.final_size += size
            for chk in union:
                self.sizes[chk] = size

    # Helper to reason about tests - pass sets of strings (corresponding to
    # checksums) and it will make a sizer with those synonym sets. Also call
    # _set_expected_unions prior to this, to set what the final result of the
    # merge is expected to be
    def _make_sizer(
        self, *syn_sets: Iterable[Set[str]]
    ) -> Tuple[RepoSizer, List[int]]:
        sizer = RepoSizer()
        for syns in syn_sets:
            assert len(syns) > 0
            # Use random object as canonical
            canonical = syns.pop()
            for chk in syns:
                assert chk in self.sizes
                rpm = _FAKE_RPM._replace(
                    checksum=Checksum(chk, chk + 'v'),
                    canonical_checksum=Checksum(canonical, canonical + 'v'),
                    size=self.sizes[chk]
                )
                sizer.visit_rpm(rpm)
        return sizer

    def _expected_chk_size_map(
        self, *ids: Iterable[str]
    ) -> Mapping[Checksum, int]:
        return {Checksum(k, k + 'v'): self.sizes[k] for k in ids}

    def test_sizer(self):
        sizer = RepoSizer()
        rpm1 = _FAKE_RPM._replace(
            checksum=Checksum('a1', 'a1v1'),
            size=1_000_000,
        )
        sizer.visit_rpm(rpm1)

        # This changes best_checksum, so a synonym will be made.
        # Note that the size is initially incorrect.
        rpm2 = _FAKE_RPM._replace(
            checksum=Checksum('a1', 'a1v1'),
            canonical_checksum=Checksum('a2', 'a2v1'),
            size=1_000,
        )
        self.assertNotEqual(rpm1.best_checksum(), rpm2.best_checksum())
        with self.assertRaisesRegex(AssertionError, ' has prior size '):
            sizer.visit_rpm(rpm2)
        sizer.visit_rpm(rpm2._replace(size=1_000_000))
        # These will also get mapped to the same synonym.
        sizer.visit_rpm(_FAKE_RPM._replace(
            checksum=Checksum('a1', 'a1v1'),
            size=1_000_000,
        ))
        sizer.visit_rpm(_FAKE_RPM._replace(
            checksum=Checksum('a2', 'a2v1'),
            size=1_000_000,
        ))
        self.assertEqual({'Rpm': 1_000_000}, sizer._get_classname_to_size())

        # Now we have two distinct checksum clusters.
        rpm3 = _FAKE_RPM._replace(
            canonical_checksum=Checksum('a4', 'a4v1'),
            checksum=Checksum('a3', 'a3v1'),
            size=1_000_000,
        )
        sizer.visit_rpm(rpm3)
        with self.assertRaisesRegex(AssertionError, ' has prior size '):
            sizer.visit_rpm(rpm3._replace(size=123))
        self.assertEqual({'Rpm': 2_000_000}, sizer._get_classname_to_size())
        # Now, they got merged again
        sizer.visit_rpm(_FAKE_RPM._replace(
            canonical_checksum=Checksum('a1', 'a1v1'),
            checksum=Checksum('a4', 'a4v1'),
            size=1_000_000,
        ))
        self.assertEqual({'Rpm': 1_000_000}, sizer._get_classname_to_size())

        # Add a couple of distinct RPMs
        sizer.visit_rpm(_FAKE_RPM._replace(
            checksum=Checksum('a1', 'a1v2'),
            size=234_000,
        ))
        sizer.visit_rpm(_FAKE_RPM._replace(
            checksum=Checksum('a1', 'a1v3'),
            size=567,
        ))
        self.assertEqual({'Rpm': 1_234_567}, sizer._get_classname_to_size())
        self.assertRegex(
            sizer.get_report('Msg'),
            '^Msg 1,234,567 bytes, by type: Rpm: 1,234,567$',
        )

    # Ensure _checksum_size is structured as we'd expect
    def test_counter_invariants(self):
        self._set_expected_unions({'a', 'b', 'c'})
        sizer = self._make_sizer({'a', 'b'}, {'a', 'c'})

        self.assertEqual(
            self._expected_chk_size_map('a', 'b', 'c'),
            sizer._type_to_counter[Rpm]._synonyms.checksum_size
        )

    # Ensure synonyms are spread across sizers when merging
    def test_merge_distinct_spread(self):
        self._set_expected_unions({'a', 'b', 'c', 'd', 'e', 'f'}, {'g', 'h'})
        sizer_a = self._make_sizer({'a', 'b', 'c'}, {'d', 'e'})
        sizer_b = self._make_sizer({'c', 'e', 'f'}, {'g', 'h'})

        sizer_a += sizer_b
        self.assertEqual(
            {'Rpm': self.final_size},
            sizer_a._get_classname_to_size()
        )
        self.assertEqual(
            self._expected_chk_size_map('a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'),
            sizer_a._type_to_counter[Rpm]._synonyms.checksum_size
        )

    # Distinct checksums are left alone
    def test_merge_distinct(self):
        self._set_expected_unions({'a', 'b'}, {'c', 'd'}, {'e', 'f'})
        sizer_a = self._make_sizer({'a', 'b'})
        sizer_b = self._make_sizer({'c', 'd'}, {'e', 'f'})

        sizer_a += sizer_b
        self.assertEqual(
            {'Rpm': self.final_size},
            sizer_a._get_classname_to_size()
        )

    def test_multiple_types(self):
        sizer_a = RepoSizer()
        sizer_b = RepoSizer()

        rpm = _FAKE_RPM._replace(
            checksum=Checksum('a1', 'a1v1'),
            size=1_000_000,
        )
        sizer_a.visit_rpm(rpm)

        repodata_a = Repodata(
            checksum=Checksum('a2', 'a2v'),
            size=3_000_000,
            location=None, build_timestamp=None,
        )
        repodata_b = Repodata(
            checksum=Checksum('a3', 'a3v'),
            size=2_000_001,
            location=None, build_timestamp=None,
        )
        sizer_b.visit_repodata(repodata_a)
        sizer_b.visit_repodata(repodata_b)
        sizer_a += sizer_b
        self.assertEqual(
            {'Rpm': 1_000_000, 'Repodata': 5_000_001},
            sizer_a._get_classname_to_size()
        )

    def test_multiple_types_same_checksum(self):
        sizer_a = RepoSizer()
        sizer_b = RepoSizer()

        rpm = _FAKE_RPM._replace(
            size=1_000_000,
            checksum=Checksum('a1', 'a1v1')
        )
        sizer_a.visit_rpm(rpm)
        repodata = Repodata(
            checksum=Checksum('a1', 'a1v1'),
            size=3_000_000,
            location=None, build_timestamp=None,
        )
        sizer_b.visit_repodata(repodata)
        sizer_a += sizer_b
        self.assertEqual(
            {'Rpm': 1_000_000, 'Repodata': 3_000_000},
            sizer_a._get_classname_to_size()
        )
