# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:build_defs.bzl", "buck_genrule")
load("//antlir/bzl:rpm_repo_snapshot.bzl", "rpm_repo_snapshot")

def test_rpm_repo_snapshot(name, kind, rpm_installers, repo_server_ports):
    bare_snapshot_dir = "__bare_snapshot_dir_for__" + name
    buck_genrule(
        name = bare_snapshot_dir,
        bash = """
        set -ue
        logfile=\\$(mktemp)
        keypair_dir=$(location //antlir/rpm/tests/gpg_test_keypair:gpg-test-keypair)
        # Only print the logs on error.
        $(exe //antlir/rpm:temp-snapshot) --kind {quoted_kind} "$OUT" \
            --gpg-keypair-dir "$keypair_dir" \
            &> "$logfile" || (cat "$logfile" 1>&2 ; exit 1)
        """.format(quoted_kind = shell.quote(kind)),
    )
    rpm_repo_snapshot(
        name = name,
        src = ":" + bare_snapshot_dir,
        storage = {
            # We have hacks to interpret this path as relative to the
            # snapshot directory, even as the snapshot is copied from `src`
            # to the build appliance.
            "base_dir": "storage",
            "key": "test",
            "kind": "filesystem",
        },
        rpm_installers = rpm_installers,
        repo_server_ports = repo_server_ports,
    )
