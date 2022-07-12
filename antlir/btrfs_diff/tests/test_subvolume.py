#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import copy

from antlir.btrfs_diff.coroutine_utils import while_not_exited
from antlir.btrfs_diff.extent import Extent
from antlir.btrfs_diff.freeze import freeze
from antlir.btrfs_diff.inode_id import InodeIDMap
from antlir.btrfs_diff.parse_dump import SendStreamItems
from antlir.btrfs_diff.rendered_tree import (
    emit_all_traversal_ids,
    emit_non_unique_traversal_ids,
    map_bottom_up,
    TraversalID,
)
from antlir.btrfs_diff.subvolume import Subvolume
from antlir.btrfs_diff.tests.deepcopy_test import DeepCopyTestCase
from antlir.btrfs_diff.tests.subvolume_utils import (
    expected_subvol_add_traversal_ids,
    InodeRepr,
)


class SubvolumeTestCase(DeepCopyTestCase):
    def _check_render(
        self, expected_ser, subvol: Subvolume, path: str = "."
    ) -> None:
        self.assertEqual(
            *[
                emit_all_traversal_ids(ser)
                for ser in (
                    expected_subvol_add_traversal_ids(expected_ser),
                    subvol.render(path.encode()),
                )
            ]
        )

    def _check_both_renders(
        self, expected_ser, subvol: Subvolume, path: str = "."
    ) -> None:
        self._check_render(expected_ser, subvol, path)
        # Always check the frozen variant, too.
        self._check_render(expected_ser, freeze(subvol), path)

    def _check_subvolume(self):
        """
        The `yield` statements in this generator allow `DeepCopyTestCase`
        to replace `ns.id_map` with a different object (either a `deepcopy`
        or a pre-`deepcopy` original from a prior run). See the docblock of
        `DeepCopyTestCase` for the details.

        This test does not try to exhaustively cover items like `chmod` and
        `write` that are applied by `IncompleteInode`, since that has its
        own unit test.  We exercise a few, to ensure that they get proxied.
        """
        si = SendStreamItems

        # Make a tiny subvolume
        cat = Subvolume.new(id_map=InodeIDMap.new(description="cat"))
        cat = yield "empty cat", cat
        self._check_both_renders(["(Dir)", {}], cat)

        cat.apply_item(si.mkfile(path=b"dog"))
        self._check_both_renders(["(Dir)", {"dog": ["(File)"]}], cat)

        cat.apply_item(si.chmod(path=b"dog", mode=0o755))
        cat = yield "cat with chmodded dog", cat
        self._check_both_renders(["(Dir)", {"dog": ["(File m755)"]}], cat)

        cat.apply_item(si.write(path=b"dog", offset=0, data="bbb"))
        cat = yield "cat with dog with data", cat
        self._check_both_renders(["(Dir)", {"dog": ["(File m755 d3)"]}], cat)

        cat.apply_item(si.chmod(path=b"dog", mode=0o744))
        self._check_both_renders(["(Dir)", {"dog": ["(File m744 d3)"]}], cat)
        with self.assertRaisesRegex(RuntimeError, "Missing ancestor "):
            cat.apply_item(si.mkfifo(path=b"dir_to_del/fifo_to_del"))

        cat.apply_item(si.mkdir(path=b"dir_to_del"))
        cat.apply_item(si.mkfifo(path=b"dir_to_del/fifo_to_del"))
        cat_final_repr = [
            "(Dir)",
            {
                "dog": ["(File m744 d3)"],
                "dir_to_del": ["(Dir)", {"fifo_to_del": ["(FIFO)"]}],
            },
        ]
        cat = yield "final cat", cat
        self._check_both_renders(cat_final_repr, cat)

        # Check some rename errors
        with self.assertRaisesRegex(RuntimeError, "makes path its own subdir"):
            cat.apply_item(si.rename(path=b"dir_to_del", dest=b"dir_to_del/f"))

        with self.assertRaisesRegex(RuntimeError, "source .* does not exist"):
            cat.apply_item(si.rename(path=b"not here", dest=b"dir_to_del/f"))

        with self.assertRaisesRegex(RuntimeError, "cannot overwrite a dir"):
            cat.apply_item(si.rename(path=b"dog", dest=b"dir_to_del"))

        cat.apply_item(si.mkdir(path=b"temp_dir"))
        cat = yield "cat with temp_dir", cat
        with self.assertRaisesRegex(RuntimeError, "only overwrite an empty d"):
            cat.apply_item(si.rename(path=b"temp_dir", dest=b"dog"))
        with self.assertRaisesRegex(RuntimeError, "since it has children"):
            cat.apply_item(si.rename(path=b"temp_dir", dest=b"dir_to_del"))

        # Cannot hardlink directories
        with self.assertRaisesRegex(RuntimeError, "Cannot .* a directory"):
            cat.apply_item(si.link(path=b"another_temp", dest=b"temp_dir"))
        cat.apply_item(si.rmdir(path=b"temp_dir"))

        # Cannot act on nonexistent paths
        with self.assertRaisesRegex(RuntimeError, "path does not exist"):
            cat.apply_item(si.chmod(path=b"temp_dir", mode=0o321))
        with self.assertRaisesRegex(RuntimeError, "source does not exist"):
            cat.apply_item(si.link(path=b"another_temp", dest=b"temp_dir"))

        # Testing the above errors caused no changes
        cat = yield "cat after error testing", cat
        self._check_both_renders(cat_final_repr, cat)

        # Make a snapshot using the hack from `SubvolumeSetMutator`
        tiger = copy.deepcopy(
            cat, memo={id(cat.id_map.inner.description): "tiger"}
        )
        tiger = yield "freshly copied tiger", tiger
        self._check_both_renders(cat_final_repr, tiger)

        # rmdir/unlink errors, followed by successful removal
        with self.assertRaisesRegex(RuntimeError, "since it has children"):
            tiger.apply_item(si.rmdir(path=b"dir_to_del"))
        with self.assertRaisesRegex(RuntimeError, "Can only rmdir.* a dir"):
            tiger.apply_item(si.rmdir(path=b"dir_to_del/fifo_to_del"))
        tiger.apply_item(si.unlink(path=b"dir_to_del/fifo_to_del"))
        with self.assertRaisesRegex(RuntimeError, "Cannot unlink.* a dir"):
            tiger.apply_item(si.unlink(path=b"dir_to_del"))
        tiger = yield "tiger after rmdir/unlink errors", tiger
        tiger.apply_item(si.rmdir(path=b"dir_to_del"))
        tiger = yield "tiger after rmdir", tiger
        self._check_both_renders(["(Dir)", {"dog": ["(File m744 d3)"]}], tiger)

        # Rename where the target does not exist
        tiger.apply_item(si.rename(path=b"dog", dest=b"wolf"))
        tiger = yield "tiger after rename", tiger
        self._check_both_renders(["(Dir)", {"wolf": ["(File m744 d3)"]}], tiger)

        # Hardlinks, and modifying the root directory
        tiger.apply_item(si.chown(path=b".", uid=123, gid=456))
        tiger.apply_item(si.link(path=b"tamaskan", dest=b"wolf"))
        tiger.apply_item(si.chmod(path=b"tamaskan", mode=0o700))
        tiger = yield "tiger after hardlink", tiger
        wolf = InodeRepr("(File m700 d3)")
        tiger_penultimate_repr = [
            "(Dir o123:456)",
            {"wolf": [wolf], "tamaskan": [wolf]},
        ]
        self._check_both_renders(tiger_penultimate_repr, tiger)

        # Renaming the same inode is a no-op
        tiger.apply_item(si.rename(path=b"tamaskan", dest=b"wolf"))
        tiger = yield "tiger after same-inode rename", tiger
        self._check_both_renders(tiger_penultimate_repr, tiger)

        # Hardlinks do not overwrite targets
        tiger.apply_item(si.mknod(path=b"somedev", mode=0o20444, dev=0x4321))
        # Freeze this 3-file filesystem to make sure that the frozen
        # `Subvolume` does not change as we evolve its parent.
        frozen_tiger = freeze(tiger)
        frozen_repr = [
            "(Dir o123:456)",
            {
                "somedev": ["(Char m444 4321)"],
                "tamaskan": [wolf],
                "wolf": [wolf],
            },
        ]
        self._check_both_renders(frozen_repr, tiger)
        # Also ensure that `emit_non_unique_traversal_ids` does what it says
        # on the tin.
        self.assertEqual(
            [
                "(Dir o123:456)",
                {
                    "somedev": ["(Char m444 4321)"],
                    "tamaskan": [[wolf.ino_repr, 0]],
                    "wolf": [[wolf.ino_repr, 0]],
                },
            ],
            emit_non_unique_traversal_ids(frozen_tiger.render()),
        )
        # *impl because otherwise we'd try to freeze a frozen `Subvolume`,
        # and my code currently does not handle that.
        self._check_render(frozen_repr, frozen_tiger)
        with self.assertRaisesRegex(RuntimeError, "Destination .* exists"):
            tiger.apply_item(si.link(path=b"wolf", dest=b"somedev"))
        tiger = yield "tiger after mkdev etc", tiger

        # A rename that overwrites an existing file.
        tiger.apply_item(si.rename(path=b"somedev", dest=b"wolf"))
        tiger = yield "tiger after overwriting rename", tiger
        self._check_both_renders(
            [
                "(Dir o123:456)",
                {"wolf": ["(Char m444 4321)"], "tamaskan": [wolf]},
            ],
            tiger,
        )

        # Graceful error on paths that cannot be resolved
        for fail_fn in [
            lambda: tiger.apply_item(si.truncate(path=b"not there", size=15)),
            lambda: tiger.apply_clone(
                si.clone(
                    path=b"not there",
                    offset=0,
                    len=1,
                    from_uuid="",
                    from_transid=0,
                    from_path=b"tamaskan",
                    clone_offset=0,
                ),
                tiger,
            ),
            lambda: tiger.apply_clone(
                si.clone(
                    path=b"tamaskan",
                    offset=0,
                    len=1,
                    from_uuid="",
                    from_transid=0,
                    from_path=b"tamaskan",
                    clone_offset=0,
                ),
                cat,
            ),  # `cat` lacks `tamaskan`
        ]:
            with self.assertRaisesRegex(RuntimeError, r" does not exist"):
                fail_fn()

        # Clones
        tiger.apply_item(si.write(path=b"tamaskan", offset=10, data=b"a" * 10))
        tiger.apply_item(si.mkfile(path=b"dolly"))
        tiger.apply_clone(
            si.clone(
                path=b"dolly",
                offset=0,
                len=10,
                from_uuid="",
                from_transid=0,
                from_path=b"tamaskan",
                clone_offset=5,
            ),
            tiger,
        )
        tiger = yield "tiger cloned from tiger", tiger
        self._check_render(
            [
                "(Dir o123:456)",
                {
                    "wolf": ["(Char m444 4321)"],
                    "tamaskan": ["(File m700 d3h7d10)"],
                    "dolly": ["(File h5d5)"],
                },
            ],
            tiger,
        )
        self._check_render(
            [
                "(Dir o123:456)",
                {
                    "wolf": ["(Char m444 4321)"],
                    "tamaskan": [
                        (
                            "(File m700 d3h7(tiger@dolly:0+5@2)d10"
                            "(tiger@dolly:5+5@0))"
                        )
                    ],
                    "dolly": [
                        (
                            "(File h5(tiger@tamaskan:5+5@0)d5"
                            "(tiger@tamaskan:10+5@0))"
                        )
                    ],
                },
            ],
            freeze(tiger),
        )
        # We're about to clone from `cat`, so allow it do be `deepcopy`d here.
        cat = yield "tiger clones from cat", cat
        self._check_both_renders(cat_final_repr, cat)

        tiger.apply_clone(
            si.clone(
                path=b"dolly",
                offset=2,
                len=2,
                from_uuid="",
                from_transid=0,
                from_path=b"dog",
                clone_offset=1,
            ),
            cat,
        )

        # The big comment below explains why we need `deepcopy_shenanigan`.
        dog_extent = cat.inode_at_path(b"dog").extent
        first_tamaskan_leaf = next(
            tiger.inode_at_path(b"tamaskan").extent.gen_trimmed_leaves()
        )[2]
        self.assertEqual(
            {Extent(content=Extent.Kind.DATA, offset=0, length=3)},
            {dog_extent, first_tamaskan_leaf},
        )
        deepcopy_shenanigan = dog_extent is not first_tamaskan_leaf

        tiger = yield "tiger cloned from cat", tiger
        self._check_render(
            [
                "(Dir o123:456)",
                {
                    "wolf": ["(Char m444 4321)"],
                    "tamaskan": ["(File m700 d3h7d10)"],
                    "dolly": ["(File h2d2h1d5)"],
                },
            ],
            tiger,
        )
        # We do not see the `cat` clone -- that requires `SubvolumeSet`.
        # However, the first 3 bytes of `tamaskan` are actually backed by
        # the same extent as `cat@dog`.  The events leading up to this
        # coincidence are:
        #  - mkfile cat@dog
        #  - write 3 bytes to offset 0 of cat@dog
        #  - snapshot cat to tiger
        #  - rename tiger@dog to tiger@wolf
        #  - hardlink tiger@wolf to tiger@tamaskan
        # Good thing you knew that a tamaskan is a breed of dog, or you'd be
        # very confused by now ;)
        #
        # The final twist is that `DeepCopyTestCase` can actually break this
        # clone connection.  The `deepcopy_original=True` subtest takes a
        # `tiger` snapshot midway from one rune, and inserts it into the
        # same position on the second run.  Of course, there is no clone
        # relationship between the `cat` of the second run, and the `tiger`
        # of the first. That's what `deepcopy_shenanigan` detects.
        self._check_render(
            [
                "(Dir o123:456)",
                {
                    "wolf": ["(Char m444 4321)"],
                    "tamaskan": [
                        "(File m700 d3"
                        + ("" if deepcopy_shenanigan else "(tiger@dolly:2+2@1)")
                        + "h7(tiger@dolly:0+2@2/tiger@dolly:4+1@6)"
                        "d10(tiger@dolly:5+5@0))"
                    ],
                    "dolly": [
                        "(File h2(tiger@tamaskan:5+2@0)d2"
                        + (
                            ""
                            if deepcopy_shenanigan
                            else "(tiger@tamaskan:1+2@0)"
                        )
                        + "h1(tiger@tamaskan:9+1@0)d5(tiger@tamaskan:10+5@0))"
                    ],
                },
            ],
            freeze(tiger),
        )

        # Mutating the snapshot leaves the parent subvol intact
        cat = yield "cat after tiger mutations", cat
        self._check_both_renders(cat_final_repr, cat)
        # Test the inode iterator
        self.assertEqual(
            ["(Dir)", "(Dir)", "(FIFO)", "(File m744 d3)"],
            sorted(repr(ino) for ino in cat.inodes()),
        )

        # Basic checks to ensure it's immutable.
        with self.assertRaisesRegex(TypeError, "NoneType.* not an iterator"):
            frozen_tiger.apply_item(si.mkfile(path=b"soup"))
        with self.assertRaisesRegex(TypeError, "no.* item deletion"):
            frozen_tiger.apply_item(si.unlink(path=b"somedev"))
        with self.assertRaisesRegex(TypeError, "no.* item deletion"):
            frozen_tiger.apply_item(si.rename(path=b"wolf", dest=b"cat"))
        with self.assertRaisesRegex(AttributeError, "no .* 'apply_item'"):
            frozen_tiger.apply_item(si.chmod(path=b"tamaskan", mode=0o644))
        # Neither the error-testing, nor changing the parent changed us.
        self._check_render(frozen_repr, frozen_tiger)

    def test_stays_frozen(self) -> None:
        """
        Freeze the yielded subvolume at every step, and ensure that the
        frozen object does not change even as we evolve its source.
        """
        subvol = None
        step_subvol_repr = []
        with while_not_exited(self._check_subvolume()) as ctx:
            while True:
                step, subvol = ctx.send(subvol)
                frozen_subvol = freeze(subvol)
                step_subvol_repr.append(
                    (step, frozen_subvol, frozen_subvol.render())
                )
        for step, frozen_subvol, frozen_repr in step_subvol_repr:
            # Sneak in some test coverage for both variants of `emit_`:
            for emit_fn in (
                emit_all_traversal_ids,
                emit_non_unique_traversal_ids,
            ):
                with self.subTest(f"frozen did not change after {step}"):
                    self.assertEqual(
                        emit_fn(frozen_repr), emit_fn(frozen_subvol.render())
                    )

    def test_subvolume(self) -> None:
        self.check_deepcopy_at_each_step(self._check_subvolume)

    def test_rendered_tree(self) -> None:
        "Miscellaneous coverage over `rendered_tree.py`."
        with self.assertRaisesRegex(RuntimeError, "Unknown type in rendered"):
            # pyre-fixme[6]: For 1st param expected `Union[Tuple[typing.Any],
            #  Tuple[typing.Any, Mapping[bytes, typing.Any]]]` but got `str`.
            map_bottom_up("not a list", lambda x: x)
        with self.assertRaisesRegex(RuntimeError, "inode list length != 1, 2"):
            # pyre-fixme[6]: For 1st param expected `Union[Tuple[typing.Any],
            #  Tuple[typing.Any, Mapping[bytes, typing.Any]]]` but got
            #  `List[Variable[_T]]`.
            map_bottom_up([], lambda x: x)
        # Ensure 'if children is None:' is equivalent to `list(ser) == 1`.
        all_equal = {
            tuple(l)
            for l in (
                # pyre-fixme[6]: For 1st param expected `Union[Tuple[typing.Any],
                #  Tuple[typing.Any, Mapping[bytes, typing.Any]]]` but got `List[str]`.
                map_bottom_up(["ino"], lambda x: x),
                # pyre-fixme[6]: For 1st param expected `Union[Tuple[typing.Any],
                #  Tuple[typing.Any, Mapping[bytes, typing.Any]]]` but got
                #  `List[Optional[str]]`.
                map_bottom_up(["ino", None], lambda x: x),
                ["ino"],
            )
        }
        self.assertEqual({("ino",)}, all_equal)
        self.assertEqual("TraversalID(11/0)", repr(TraversalID(11)))
