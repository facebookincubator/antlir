load("@bazel_skylib//lib:shell.bzl", "shell")
load("//fs_image/bzl:oss_shim.bzl", "buck_genrule")
load("//fs_image/bzl:rpm_repo_snapshot.bzl", "rpm_repo_snapshot")

def test_rpm_repo_snapshot(name, kind, rpm_installers):
    bare_snapshot_dir = "__bare_snapshot_dir_for__" + name
    buck_genrule(
        name = bare_snapshot_dir,
        out = "unused",
        bash = """
        set -ue
        logfile=\\$(mktemp)
        keypair_dir=$(location //fs_image/rpm:gpg-test-keypair)
        # Only print the logs on error.
        $(exe //fs_image/rpm:temp-snapshot) --kind {quoted_kind} "$OUT" \
            --gpg-keypair-dir "$keypair_dir" \
            &> "$logfile" || (cat "$logfile" 1>&2 ; exit 1)
        """.format(quoted_kind = shell.quote(kind)),
        fs_image_internal_rule = True,
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
    )
