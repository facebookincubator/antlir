# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:oss_shim.bzl", "buck_command_alias")
load("//antlir/bzl:rpm_repo_snapshot.bzl", "rpm_repo_snapshot")

def snapshot(
        name,
        src,
        storage_config,
        dnf_conf,
        gpg_key_allowlist_dir,
        rpm_installers = ("dnf",)):
    rpm_repo_snapshot(
        name = name,
        src = src,
        rpm_installers = rpm_installers,
        storage = storage_config,
    )

    # Command to run from the root of the oss repo to snapshot the given repositories
    buck_command_alias(
        name = "snapshot-" + name,
        args = [
            "--snapshot-dir=snapshot/" + src,
            "--gpg-key-allowlist-dir=snapshot/" + gpg_key_allowlist_dir,
            '--db={"kind": "sqlite", "db_path": "snapshot/snapshots.sql3"}',
            "--threads=16",
            "--storage={}".format(repr(storage_config)),
            "--one-universe-for-all-repos=" + name,
            "--dnf-conf=$(location {})".format(dnf_conf),
            "--yum-conf=$(location {})".format(dnf_conf),
        ],
        exe = "//antlir/rpm:snapshot-repos",
    )

def storage_config(distro, release):
    return {
        "bucket": "antlir",
        "key": "s3",
        "kind": "s3",
        "prefix": "snapshots/{}/{}".format(distro, release),
        "region": "us-east-2",
    }
