# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def expand_members(
    groups: dict[str, list[str]],
    *,
    arch_names: list[str],
    current_arch: str,
) -> list[str]:
    """Expand `members_compressed` groups into `{name}_{version}_{arch}` NEVRAs.

    The encoder is `compress_members` in the snapshot binary
    (`snapshot/src/snapshot/buck_target.rs`); keep the two in sync.
    `groups` maps a shared package-name prefix (e.g.
    `asterisk-core-sounds-en-`, or `""` for ungrouped stems) to one
    `{rest}|{arches}` entry per stem, where the full stem is
    `prefix + rest` and `arches` names the arches the stem exists for
    with the letters `a` (arch-independent), `x` (x86) and `r` (arm)
    in that order, referring to the `arch_names` spellings.

    Only NEVRAs for the arch-independent spelling and for
    `current_arch` are returned, mirroring what the plain `members`
    list plus `select()` resolved to. `current_arch` is the binary
    arch spelling for the current platform (e.g. `amd64`), or the
    empty string when the platform matches neither binary arch.
    """
    out: list[str] = []
    for prefix, entries in groups.items():
        for entry in entries:
            rest, arches = entry.split("|")
            stem = prefix + rest
            if "a" in arches:
                out.append(stem + "_" + arch_names[0])
            if "x" in arches and current_arch == arch_names[1]:
                out.append(stem + "_" + arch_names[1])
            if "r" in arches and current_arch == arch_names[2]:
                out.append(stem + "_" + arch_names[2])
    return out
