---
id: btrfs_diff
title: btrfs_diff/
---

## Improvements to the present codebase / tech debt

- The current way of handling of clone references in our filesystem output (`ChunkClone`s) is deeply problematic because it is quadratic in the number of times an extent is cloned. So, the representation becomes unusable as the number of represented snapshots grows. The rationale for this quadratic hack is discussed in `extents_to_chunks.py`. However, underneath the hood, all the clones share a single Extent object, which means we **could** treat this in a way that's linear in the number of clones. The likely best approach would be akin to what we already do for hardlinks. For chunks, we'd introduce some kind of artificial "block" or "extent" numbering and deterministically populate it at serialization time, as well as for user input. Refer to `serialize_subvol` and `serialized_subvol_add_fake_inode_ids` for the hardlink example.

- It is problematic that we have frozen & unfrozen versions of everything, with subtle distinctions in semantics besides read-only vs read-write. For example, it is silly that I need to `freeze` to `assert_valid_and_complete`. We have this wart for two reasons:

  1.  cloned-extent-finding is not an online algorithm, so we have to run `extents_to_chunks` whenever we want to see what clones what. Of course, the moment we make this index, we'd better make sure the rest of the structure is frozen, so that mutations don't invalidate the index. It may well be worth making the "what clones what?" index update on each mutation, if it can be done in a way that is simple and not grossly inefficient. In a world like this, we no longer need `freeze` support -- `deepcopy` support is enough.

  1.  We cannot easily share representation (and thus mehtods like `assert_valid_and_complete` between the mutable and immutable versions of the data. Finishing to build out `deepfrozen` is a path towards fixing that. If done correctly, the mutable & immutable data structures would become interchangeable for all read-only operations. This would make it much less important to fix (i).

- Instead of `repr` printing the device ID, print `major,minor`.

- It'd be good to rename `Extent` to something more descriptive, like `ExtentsLog` (name to-be-improved), and to rename our current `Chunk` to be `Extent`.

- The current implementation of `InodeIDMap` feels more complex (and inefficient) than it must be. Streamline it once that's needed. Concrete points:

  - `get_children` should maybe just return names, not full paths?

- Consistently use `sendstream` in filenames instead of `send_stream`. Rationale: `send-stream` is a compound noun, the underscore makes it confusable with `send stream`.

- Consider renaming the `dest` field of `link` to be `to`. This conflicts with `btrfs receive --dump` nomenclature, but agrees with `btrfs-progs` in-code field names.

- Add an explicit test & TARGET for `TraversalIDMaker`?

- `SubvolumeSet.render` and `get_by_rendered_id` reaching into `_InnerInodeIDMap` is pretty gross.

- `SubvolumeSet` claims it's `deepcopy`able, but there is no direct test.

- `inode_utils.py` should have a small, simple, explicit test instead of being covered by the integration test.

- Our send-stream ingestion should handle multiple concatenated sendstreams, since btrfs has an "end" command. This makes many use-cases easier and more UNIXy, e.g. `sendstreams_to_json_subvolumes.py`. Ensure that any user of the ingestion APIs is ready to handle multiple send- streams.

- Add a sendstream binary writer, confirm that parse-serialize produces bit-identical output (thus ensuring we lose nothing).

## Ideas for the future

- While this toolchain already incidentally tests many aspects of the kernel & `btrfs-progs` implementations (and identified around 10 bugs during its development), it has the potential to be used much more systematically to this effect. One generic mechanism for testing diffs is "endpoint testing", as follows:

  - The tester writes a program that performs a sequence of filesystem operations (this can also be `fsstress` or the `xfstests` regression test suite).

  - The filesystem operations are sent & received using regular tooling.

  - A `btrfs_diff` program also consumes the send-streams, and constructs a `SubvolumeSet`. It then traverses its `Inode`s, and and issues syscalls against the original filesystem, as well as the received filesystem to verify all the data that it is aware of, including:
    - file type, owner, mode, xattrs, link destinations, device #s
    - hardlinks -- use something like `TraversalID` to make the physical inode numbers relevant to our in-memory tree.
    - cloned extents -- akin to hardlinks, but use `fiemap` to obtain physical block numbers.

  Since `btrfs_diff` is focused on completeness of representation, this approach is better ad-hoc filesystem comparisons in that it can be relied on to test all the data `btrfs_diff` tracks.

  Additionally, `btrfs_diff` is in effect a parallel implementation of `btrfs receive` together with an underlying filesystem, which provides a meaningful sanity-check on userspace and on the kernel.

  The end-game for such a kernel testing effort might even involve putting up the `btrfs_diff` library for inclusion into `btrfs-progs` (this might require a better name :).

- Add support for handling incremental sendstreams. See the note in `sendstream_has_loop_device.py` about adding the capability to construct a `Subvolume` with special placeholder objects for the dependencies, which we infer must be supplied by a parent subvolume. This is relevant to the image compiler, since it needs to be able to reason about the effects of running a sandboxed build step on the final image.
