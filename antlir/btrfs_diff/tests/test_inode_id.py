#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import random
from types import SimpleNamespace

from ..freeze import freeze, frozendict
from ..inode_id import (
    _PathEntry,
    _ReversePathEntry,
    _ROOT_REVERSE_ENTRY,
    InodeID,
    InodeIDMap,
)
from .deepcopy_test import DeepCopyTestCase


class InodeIDTestCase(DeepCopyTestCase):
    # pyre-fixme[3]: Return type must be annotated.
    def _check_id_and_map(self):
        """
        The `yield` statements in this generator allow `DeepCopyTestCase`
        to replace `ns.id_map` with a different object (either a `deepcopy`
        or a pre-`deepcopy` original from a prior run). See the docblock of
        `DeepCopyTestCase` for the details.
        """
        INO1_ID = 1
        INO2_ID = 2
        STEP_MADE_ANON_INODE = "made anon inode"  # has special 'replace' logic

        # Shared scope between the outer function and `maybe_replace_map`
        # Stores inode objects pointing at the un-frozen (mutable) `id_map`.
        mut_ns = SimpleNamespace()

        # pyre-fixme[53]: Captured variable `STEP_MADE_ANON_INODE` is not annotated.
        # pyre-fixme[53]: Captured variable `mut_ns` is not annotated.
        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        def maybe_replace_map(id_map, step_name):
            new_map = yield step_name, id_map
            if new_map is not id_map:
                # If the map was replaced, we must fix up our inode objects.
                # pyre-fixme[16]: `None` has no attribute `get_id`.
                mut_ns.ino_root = new_map.get_id(b".")
                if hasattr(mut_ns, "ino1"):
                    mut_ns.ino1 = new_map.get_id(b"a")
                if hasattr(mut_ns, "ino2"):
                    if step_name == STEP_MADE_ANON_INODE:
                        # pyre-fixme[16]: `None` has no attribute `inner`.
                        mut_ns.ino2 = InodeID(id=2, inner_id_map=new_map.inner)
                    else:
                        # we add a/c later, remove a/c earlier, this is enough
                        mut_ns.ino2 = new_map.get_id(b"a/d")
            return new_map  # noqa: B901

        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        def unfrozen_and_frozen_impl(id_map, mut_ns):
            "Run all checks on the mutable map and on its frozen counterpart"
            yield id_map, mut_ns
            frozen_map = freeze(id_map)
            yield frozen_map, SimpleNamespace(
                **{
                    # Avoiding `frozen_map.get_id(...id_map.get_paths(v))`
                    # since that won't work with an anonymous inode.
                    k: InodeID(id=v.id, inner_id_map=frozen_map.inner)
                    for k, v in mut_ns.__dict__.items()
                    if v is not None  # for `mut_ns.ino2 = new_map.get_id`
                }
            )

        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        def unfrozen_and_frozen(id_map, mut_ns):
            res = list(unfrozen_and_frozen_impl(id_map, mut_ns))
            # The whole test would be useless if the generator didn't return
            # any items, so add some paranoia around that.
            self.assertEqual(2, len(res), repr(res))
            return res

        id_map = yield from maybe_replace_map(InodeIDMap.new(), "empty")

        # Check the root inode
        # pyre-fixme[16]: `SimpleNamespace` has no attribute `ino_root`.
        # pyre-fixme[16]: `None` has no attribute `get_id`.
        mut_ns.ino_root = id_map.get_id(b".")
        for im, ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual(".", repr(ns.ino_root))
            self.assertEqual({b"."}, im.get_paths(ns.ino_root))
            self.assertEqual(set(), im.get_children(ns.ino_root))

        # Make a new ID with a path
        # pyre-fixme[16]: `SimpleNamespace` has no attribute `ino1`.
        # pyre-fixme[16]: `None` has no attribute `add_dir`.
        # pyre-fixme[16]: `None` has no attribute `next`.
        mut_ns.ino1 = id_map.add_dir(id_map.next(), b"./a/")
        id_map = yield from maybe_replace_map(id_map, "made a")
        for im, ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual(INO1_ID, ns.ino1.id)
            self.assertIs(im.inner, ns.ino1.inner_id_map)
            self.assertEqual("a", repr(ns.ino1))
            self.assertEqual({b"a"}, im.get_children(ns.ino_root))
            self.assertEqual(set(), im.get_children(ns.ino1))

        # Anonymous inode, then add multiple paths
        # pyre-fixme[16]: `SimpleNamespace` has no attribute `ino2`.
        mut_ns.ino2 = id_map.next()  # initially anonymous
        id_map = yield from maybe_replace_map(id_map, STEP_MADE_ANON_INODE)
        for im, ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual(INO2_ID, ns.ino2.id)
            self.assertIs(im.inner, ns.ino2.inner_id_map)
            self.assertEqual("ANON_INODE#2", repr(ns.ino2))
        # pyre-fixme[16]: `None` has no attribute `add_file`.
        id_map.add_file(mut_ns.ino2, b"a/d")
        id_map = yield from maybe_replace_map(id_map, "added a/d name")
        for im, ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual({b"a/d"}, im.get_children(ns.ino1))
            self.assertEqual({b"a/d"}, im.get_paths(ns.ino2))
            self.assertIsNone(im.get_children(ns.ino2))
        id_map.add_file(mut_ns.ino2, b"a/c")
        id_map = yield from maybe_replace_map(id_map, "added a/c name")
        for im, ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual({b"a/c", b"a/d"}, im.get_children(ns.ino1))
            self.assertEqual({b"a/c", b"a/d"}, im.get_paths(ns.ino2))
            self.assertEqual("a/c,a/d", repr(ns.ino2))
        # Try removing from the frozen map before changing the original one.
        with self.assertRaisesRegex(
            TypeError, "'frozendict' object does not support item deletion"
        ):
            freeze(id_map).remove_path(b"a/c")
        # pyre-fixme[16]: `None` has no attribute `remove_path`.
        self.assertIs(mut_ns.ino2, id_map.remove_path(b"a/c"))
        saved_frozen_map = freeze(id_map)  # We'll check this later
        id_map = yield from maybe_replace_map(id_map, "removed a/c name")
        for im, ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual({b"a/d"}, im.get_children(ns.ino1))
            self.assertEqual({b"a/d"}, im.get_paths(ns.ino2))
            self.assertEqual("a/d", repr(ns.ino2))

            self.assertEqual({b"a"}, im.get_children(ns.ino_root))

        # Look-up by ID
        for (im, ns), check_same in zip(
            unfrozen_and_frozen(id_map, mut_ns),
            # `is` comparison would be harder to implement for the frozen
            # variant, and meaningless because we just constructed it.
            [self.assertIs, self.assertEqual],
        ):
            check_same(ns.ino1, im.get_id(b"a"))
            check_same(ns.ino2, im.get_id(b"a/d"))

        # Cannot remove non-empty directories
        with self.assertRaisesRegex(RuntimeError, "remove b'a'.*has children"):
            id_map.remove_path(b"a")

        # Test a rename that fails on the add after the remove. Note that
        # this does not affect any subsequent tests -- a/d still exists.
        with self.assertRaisesRegex(RuntimeError, "Adding #2 to .* has #1"):
            # pyre-fixme[16]: `None` has no attribute `rename_path`.
            id_map.rename_path(b"a/d", b"a")

        # Check that we clean up empty path sets
        for im, ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertIn(ns.ino2.id, im.inner.id_to_reverse_entries)
        self.assertIs(mut_ns.ino2, id_map.remove_path(b"a/d"))
        id_map = yield from maybe_replace_map(id_map, "removed a/d name")
        for im, _ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertNotIn(INO2_ID, im.inner.id_to_reverse_entries)

        for im, _ns in unfrozen_and_frozen(id_map, mut_ns):
            # Catch str/byte mixups
            with self.assertRaises(TypeError):
                im.get_id("a")
            with self.assertRaises(TypeError):
                im.remove_path("a")
            with self.assertRaises(TypeError):
                im.add_file("b")

            # Other errors
            with self.assertRaisesRegex(RuntimeError, "Wrong map for .* #17"):
                im.get_paths(
                    InodeID(id=17, inner_id_map=InodeIDMap.new().inner)
                )
            # Since `im` may be frozen, we can't actually count from it
            fake_ino_id = InodeID(id=1337, inner_id_map=im.inner)
            with self.assertRaisesRegex(ValueError, "Need relative path"):
                im.add_dir(fake_ino_id, b"/a/e")
            with self.assertRaisesRegex(RuntimeError, "Missing ancestor "):
                im.add_file(fake_ino_id, b"b/c")

        # This error differs between unfrozen & frozen:
        with self.assertRaisesRegex(
            RuntimeError, "Adding #3 to b'a' which has #1"
        ):
            id_map.add_dir(id_map.next(), b"a")
        with self.assertRaisesRegex(
            TypeError, "'NoneType' object is not an iterator"
        ):
            freeze(id_map).next()

        # OK to remove since it's now empty
        id_map.remove_path(b"a")
        id_map = yield from maybe_replace_map(id_map, "removed a")
        for im, _ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual(
                frozendict({0: {_ROOT_REVERSE_ENTRY}}),
                im.inner.id_to_reverse_entries,
            )
            self.assertEqual(
                _PathEntry(
                    id=InodeID(id=0, inner_id_map=im.inner), name_to_child={}
                ),
                im.root,
            )

        # Test renaming directories
        id_map.add_dir(id_map.next(), b"x")
        id_map.add_dir(id_map.next(), b"x/y")
        id_map.add_file(id_map.next(), b"x/y/z")
        id_map.add_dir(id_map.next(), b"u")
        id_map.add_dir(id_map.next(), b"u/v")
        id_map.add_file(id_map.get_id(b"x/y/z"), b"u/v/w")  # hardlink to z
        id_map = yield from maybe_replace_map(id_map, "created x/y/z, u/v/w")
        for im, _ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual(
                {b"x/y/z", b"u/v/w"}, im.get_paths(im.get_id(b"x/y/z"))
            )
            self.assertEqual({b"x/y/z"}, im.get_children(im.get_id(b"x/y")))
        id_map.rename_path(b"u/v", b"x/y/v")
        id_map.rename_path(b"x", b"x1")
        id_map = yield from maybe_replace_map(id_map, "renamed x & v")
        for im, _ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual(
                {b"x1/y/z", b"x1/y/v/w"}, im.get_paths(im.get_id(b"x1/y/z"))
            )
            self.assertEqual(
                {b"x1/y/z", b"x1/y/v"}, im.get_children(im.get_id(b"x1/y"))
            )
            # `get_children` promises to return None for files.
            self.assertIsNone(im.get_children(im.get_id(b"x1/y/z")))

        # Tests for `_reverse_entry_matches_path_parts`: Given an InodeID,
        # look up the `ReversePathEntry` that corresponds to a given path.
        #
        # (1) Let us specifically aims to cover the cases when, of the path
        # and the `ReversePathEntry`, one is a suffix of the other.  For
        # example: `e/d/c/b/a` and `c/b/a`.
        # pyre-fixme[16]: `SimpleNamespace` has no attribute `ino_id`.
        mut_ns.ino_id = id_map.next()
        # To see such suffixes we must get lucky with iteration order of the
        # reverse entries `set`.  The solution is to have many entries,
        # randomly inserted into the `set`, ensuring that we hit both cases.
        depths = list(range(20))  # Raise this until our coverage isn't flaky.
        random.shuffle(depths)
        for depth in depths:
            path = b""
            for i in range(depth + 1, 0, -1):
                if path:
                    path += b"/"
                path += str(i).encode()
                id_map.add_dir(id_map.next(), path)
            id_map.add_file(mut_ns.ino_id, path + b"/0")
        id_map = yield from maybe_replace_map(id_map, "made suffixy hardlinks")
        # Each `remove` has a chance of hitting either "suffix" relationship
        # before finding the right `reverse_entry`.  Randomize the order to
        # make sure our odds are "as expected", untained by systematic bias.
        random.shuffle(depths)
        for depth in depths:
            id_map.remove_path(
                b"/".join(str(i).encode() for i in range(depth + 1, -1, -1))
            )
        id_map = yield from maybe_replace_map(id_map, "removed hardlinks")
        # pyre-fixme[16]: `None` has no attribute `inner`.
        mut_ns.ino_id = InodeID(id=mut_ns.ino_id.id, inner_id_map=id_map.inner)
        # pyre-fixme[16]: `None` has no attribute `get_paths`.
        self.assertEqual(set(), id_map.get_paths(mut_ns.ino_id))
        # (2) Let's try a few hardlinks, neither a suffix of the other, to
        # make sure we hit the "different paths" branch.
        num_diff_paths = 20  # Raise this until our coverage isn't flaky.
        for i in range(num_diff_paths):
            id_map.add_file(mut_ns.ino_id, f"lala{i}".encode())
        for im, ns in unfrozen_and_frozen(id_map, mut_ns):
            self.assertEqual(
                {f"lala{i}".encode() for i in range(num_diff_paths)},
                im.get_paths(ns.ino_id),
            )
        for i in range(num_diff_paths):
            id_map.remove_path(f"lala{i}".encode())
        self.assertEqual(set(), id_map.get_paths(mut_ns.ino_id))

        # Test some more errors
        with self.assertRaisesRegex(RuntimeError, "foo''s parent.*is a file"):
            id_map.get_id(b"x1/y/z/foo/bar")
        with self.assertRaisesRegex(RuntimeError, "Cannot remove the root"):
            id_map.remove_path(b".")
        with self.assertRaisesRegex(RuntimeError, "Cannot remove non-exist"):
            id_map.remove_path(b"potato")
        with self.assertRaisesRegex(RuntimeError, "Cannot remove non-exist"):
            id_map.remove_path(b"potato")
        with self.assertRaisesRegex(RuntimeError, "non-file hardlink"):
            id_map.add_dir(id_map.get_id(b"x1/y/z"), "x1/y/z2")
        with self.assertRaisesRegex(RuntimeError, "non-file hardlink"):
            id_map.add_file(id_map.get_id(b"x1/y/v"), "x1/y/v2")
        with self.assertRaisesRegex(RuntimeError, "parent .* is a file"):
            id_map.add_file(id_map.next(), b"x1/y/z/foo")

        # Even though we changed `id_map` a lot, `saved_frozen` is still
        # in the same state where we took the snapshot.
        self.assertIsNone(saved_frozen_map.inode_id_counter)
        self.assertEqual(
            _PathEntry(
                id=InodeID(id=0, inner_id_map=saved_frozen_map.inner),
                name_to_child={
                    b"a": _PathEntry(
                        id=InodeID(
                            id=INO1_ID, inner_id_map=saved_frozen_map.inner
                        ),
                        name_to_child={
                            b"d": _PathEntry(
                                id=InodeID(
                                    id=INO2_ID,
                                    inner_id_map=saved_frozen_map.inner,
                                ),
                                name_to_child=None,
                            )
                        },
                    )
                },
            ),
            saved_frozen_map.root,
        )
        self.assertEqual("", saved_frozen_map.inner.description)
        self.assertEqual(
            {
                0: {_ROOT_REVERSE_ENTRY},
                INO1_ID: {_ReversePathEntry(name=b"a", parent_int_id=0)},
                INO2_ID: {_ReversePathEntry(name=b"d", parent_int_id=INO1_ID)},
            },
            saved_frozen_map.inner.id_to_reverse_entries,
        )

    def test_inode_id_and_map(self) -> None:
        self.check_deepcopy_at_each_step(self._check_id_and_map)

    def test_description(self) -> None:
        cat_map = InodeIDMap.new(description="cat")
        self.assertEqual(
            "cat@food", repr(cat_map.add_file(cat_map.next(), b"food"))
        )

    def test_hashing_and_equality(self) -> None:
        maps = [InodeIDMap.new() for i in range(100)]
        hashes = {hash(m.get_id(b".")) for m in maps}
        self.assertNotEqual({next(iter(hashes))}, hashes)
        # Even 5 collisions out of 100 is too many, but the goal is to avoid
        # flaky tests at all costs.
        self.assertGreater(len(hashes), 95)

        id1 = InodeIDMap.new().get_id(b".")
        # pyre-fixme[16]: Optional type has no attribute `inner_id_map`.
        id2 = InodeID(id=0, inner_id_map=id1.inner_id_map)
        self.assertEqual(id1, id2)
        self.assertEqual(hash(id1), hash(id2))
