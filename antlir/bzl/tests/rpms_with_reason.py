# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This tool helps control package bloat in your image layers.

Usage:

    image.layer(
        name = "your-layer-rpms-with-reason",
        parent_layer = ":your-layer",
        features = [feature.install_buck_runnable(
            "//antlir/bzl/tests:rpms-with-reason",
            "/rpms-with-reason",
        )],
    )
    command_alias(
        name = "print-your-layer-rpms",
        exe = "//antlir/nspawn_in_subvol:run",
        args = [
            "--layer=$(location :base.c8.rc-rpms-with-reason)",
            "--user=root",
            "--",
            "/rpms-with-reason",
            # Pro-tip: put these in a list variable and share it with the
            # `image.rpms_install` feature making your layer.
            "WANTED-RPM1",
            "WANTED-RPM2",
        ],
    )

    buck run :print-your-layer-rpms > my-rpm-list

This outputs a `image.test_rpm_names`-compatible list of RPMs installed in
`:your-layer`, one per line, with a TAB-separated annotation showing whether
this RPM is in one of these categories:
  - a dependency of a protected RPM (e.g. `systemd`)
  - a dependency of a wanted RPM you specified on the command-line
  - a wanted RPM
  - none of the above -- marked "NOT REQUIRED"

The annotation for removable packages also estimates the container size
savings from removing that package, its dependents, and its unused
dependencies.  Caveat: this assumes the removal of the dependencies you
marked as WANTED.

Future: It would be pretty easy to let this derive wanted RPMs from
`feature` targets.
"""

import re
import subprocess


def print_required_by(rpm, wanted, required_by, cost=None) -> None:
    notes = []
    if rpm in wanted:
        notes.append("wanted")
        required_by = required_by - {rpm}
    if required_by:
        notes.append("required by: " + " ".join(sorted(required_by)))
    if not notes:
        notes.append("NOT REQUIRED")
    elif cost:
        notes.append("remove to free: " + cost)
    print(f"{rpm}\t{'; '.join(notes)}")


def print_rpms_with_reason(wanted_rpms) -> None:
    wanted = set(wanted_rpms)
    # Bug alert: I was too lazy to make this handle packages that exist in
    # both i686 & x86_64 architectures, but it should be OK for our usage.
    rpms = (
        subprocess.check_output(
            ["rpm", "-qa", "--queryformat", "%{NAME}\n"],
            text=True,
        )
        .strip()
        .split("\n")
    )
    for rpm in sorted(set(rpms)):
        if rpm == "gpg-pubkey":
            print(f"{rpm}\tfor RPM signature checking")
            continue

        p = subprocess.run(
            ["dnf", "remove", "--assumeno", rpm],
            stderr=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            check=False,
        )
        m = re.search(
            " removing the following protected packages: (.*)\n", p.stderr
        )
        if m:
            print_required_by(rpm, wanted, {p for p in m.group(1).split() if p})
            continue

        if re.search("\nRemove +[0-9]+ Packages?\n", p.stdout):
            removed = [
                l.split()[0] for l in p.stdout.split("\n") if l.startswith(" ")
            ]
            assert "Package" == removed[0], removed[0]
            removed = removed[1:]
            print_required_by(
                rpm,
                wanted,
                wanted.intersection(removed),
                # pyre-fixme[16]: Optional type has no attribute `group`.
                cost=re.search("\nFreed space: (.*)\n", p.stdout).group(1),
            )
            continue

        raise AssertionError(p)


if __name__ == "__main__":
    import sys

    print_rpms_with_reason(sys.argv[1:])
