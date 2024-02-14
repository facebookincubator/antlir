# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from functools import total_ordering
from typing import NamedTuple, Optional

from antlir.rpm.rpm_metadata import compare_rpm_versions, RpmMetadata


@total_ordering
class SortableENVRA(NamedTuple):
    """
    Epoch and name can be `None` to represent wildcards (details below).

    The intended application of the sort is for diff-stable serialization of
    ENVRA IDs, so it does NOT do what you expect. Namely, it will sort:
      - packages with different names or architectures,
      - sort unknown (`None`) `.epoch` and `.name` values, so long as we're
        not comparing `None` with non-`None`.

    If you want plain `rpm`-compatible comparison of versions, just call
    `compare_rpm_versions(a.as_rpm_metadata(), b.as_rpm_metadata())`.
    """

    # Unlike RPM convention, None means "wildcard", it does not mean 0.  A
    # wildcard must be resolved to a concrete epoch by looking it up in an
    # RPM DB.
    epoch: Optional[int]
    # Set this to `None` to make an EVRA that can be applied to multiple
    # packages in a package group.
    name: Optional[str]
    version: str
    release: str
    arch: str

    # Use `as_rpm_metadata` for public consumption.  The private version
    # here allows comparing wildcard epochs because we only use it after
    # checking that we're not comparing `None` with non-`None`.
    def _as_rpm_metadata(self) -> RpmMetadata:
        return RpmMetadata(
            # Not used for sorting here, `compare_rpm_versions` refuses to
            # compare different names.  As a side-effect, `None` vs non-`None`
            # comparisons are also prohibited.
            #
            # pyre-fixme[6]: Expected `str` for 1st param but got
            # `Optional[str]`.
            name=self.name,
            # We check this is not `None` in `as_rpm_metadata`, and check
            # for heterogeneous comparisons in `_compare`.
            # pyre-fixme[6]: Expected `int` for 2nd param but got `Optional[int]`.
            epoch=self.epoch,
            version=self.version,
            release=self.release,
        )

    # Enables comparison of versions via `compare_rpm_versions`.
    def as_rpm_metadata(self) -> RpmMetadata:
        # Allowing a `None` vs non-`None` comparison would be wrong.
        #
        # Future: move the check for these comparisons out of this class
        # and into `compare_rpm_versions`.
        if self.epoch is None or self.arch is None:
            raise TypeError(
                "Cannot use `as_rpm_metadata()` with wildcard epoch or arch: " f"{self}"
            )
        return self._as_rpm_metadata()

    def _compare(self, other: "SortableENVRA") -> int:
        # It makes no sense to compare wildcard with non-wildcard because it
        # amounts to comparing different data types.  All elements of a
        # `SortableENVRA` collections should have wildcards in this field,
        # or the field should be concrete throughout.
        if (self.name is None) ^ (other.name is None):
            raise TypeError(
                f"Cannot compare concrete name with wildcard: {self} {other}"
            )
        if (self.arch is None) ^ (other.arch is None):
            raise TypeError(
                f"Cannot compare concrete arch with wildcard: {self} {other}"
            )

        # Sort lexicographically by name, then architecture
        self_key = (self.name, self.arch)
        other_key = (other.name, other.arch)

        if self_key > other_key:
            return 1
        elif self_key == other_key:
            # Same rationale as for the `.name` test above.
            if (self.epoch is None) ^ (other.epoch is None):
                raise TypeError(
                    f"Cannot compare int epoch with wildcard: {self} {other}"
                )
            return compare_rpm_versions(
                self._as_rpm_metadata(), other._as_rpm_metadata()
            )
        elif self_key < other_key:
            return -1

        raise AssertionError(f"Bad name/arch keys: {self_key} {other_key}")

    def __eq__(self, other: "SortableENVRA") -> bool:
        return self._compare(other) == 0

    # pyre-fixme[14]: `__lt__` overrides method defined in `tuple`
    # inconsistently.
    def __lt__(self, other: "SortableENVRA") -> bool:
        return self._compare(other) < 0

    def to_versionlock_line(self) -> str:
        if self.epoch is None or self.name is None or self.arch is None:
            raise ValueError(f"Versionlock needs concrete name & epoch & arch: {self}")
        # Our `yum_dnf_versionlock.py` expects TAB-separated ENVRAs.
        return "\t".join(
            [str(self.epoch), self.name, self.version, self.release, self.arch]
        )

    def __repr__(self) -> str:
        epoch = "*" if self.epoch is None else self.epoch
        name = "*" if self.name is None else self.name
        arch = "*" if self.arch is None else self.arch
        return f"{epoch}:{name}-{self.version}-{self.release}.{arch}"


# As a type-hint, this alias represents the fact that the `name` must be
# `None`.  Future: should this be a proper, separate type?
SortableEVRA = SortableENVRA
