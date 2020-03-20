load("@bazel_skylib//lib:shell.bzl", "shell")
load("//fs_image/bzl:oss_shim.bzl", "buck_genrule")
load("//fs_image/bzl:rpm_repo_snapshot.bzl", "rpm_repo_snapshot")

def test_rpm_repo_snapshot(name, kind, yum_dnf):
    bare_snapshot_dir = "__bare_snapshot_dir_for__" + name
    buck_genrule(
        name = bare_snapshot_dir,
        out = "unused",
        bash = """
        set -ue
        logfile=\\$(mktemp)
        # Only print the logs on error.
        $(exe //fs_image/rpm:temp-snapshot) --kind {quoted_kind} "$OUT" \
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
        yum_dnf = yum_dnf,
    )
