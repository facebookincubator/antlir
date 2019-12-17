load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:shell.bzl", "shell")
load(":image.bzl", "image")
load(":maybe_export_file.bzl", "maybe_export_file")
load(":oss_shim.bzl", "buck_genrule")
load(":target_tagger.bzl", "mangle_target")

# This constant is duplicated in `yum_using_build_appliance`.
RPM_SNAPSHOT_BASE_DIR = "rpm-repo-snapshot"

# Hack alert: If you pass `storage["kind"] == "filesystem"`, and its
# `"base_dir"` is relative, it will magically be interpreted to be relative
# to the snapshot dir, both by this code, and later by `yum-from-snapshot`
# that runs in the build appliance.
def rpm_repo_snapshot(name, src, storage):
    # For tests, we want relative `base_dir` to point into the snapshot dir.
    cli_storage = storage.copy()
    if cli_storage["kind"] == "filesystem" and \
       not cli_storage["base_dir"].startswith("/"):
        cli_storage["base_dir"] = "$(location {})/{}".format(
            src,
            cli_storage["base_dir"],
        )
    buck_genrule(
        name = name,
        out = "unused",
        bash = '''\
set -ue -o pipefail -o noclobber

# Rebuild RPM snapshots if this bzl (or its dependencies) change
echo $(location //fs_image/bzl:rpm_repo_snapshot) > /dev/null

cp --no-target-directory -r $(location {src}) "$OUT"

$(exe //fs_image/rpm/storage:cli) --storage {quoted_storage_cfg} \
    get "\\$(cat "$OUT"/snapshot.storage_id)" > "$OUT"/snapshot.sql3

# Write-protect the SQLite DB because it's common to use the `sqlite3` to
# inspect it, and it's otherwise very easy to accidentally mutate the build
# artifact, which is a big no-no.
chmod a-w "$OUT"/snapshot.sql3

echo {quoted_yum_sh} > "$OUT"/yum
chmod u+x "$OUT"/yum
        '''.format(
            src = maybe_export_file(src),
            quoted_storage_cfg = shell.quote(struct(**cli_storage).to_json()),
            # Hack alert: this refers to a sibling `yum-from-snapshot`,
            # which is NOT part of this target.  The reason for this is that
            # `image.install` currently lacks support for `buck run`nable
            # directories (a comment in `InstallFileItem` explains why).  So
            # instead, we have `install_rpm_repo_snapshot` inject the
            # buck-runnable binary.
            #
            # NB: At present, this is not **quite** CLI-compatible with `yum`.
            quoted_yum_sh = shell.quote('''\
#!/bin/bash -ue
set -o pipefail -o noclobber
my_path=\\$(readlink -f "$0")
my_dir=\\$(dirname "$my_path")
exec "$my_dir"/yum-from-snapshot \
    --snapshot-dir "$my_dir" --storage {quoted_storage_cfg} "$@"
            '''.format(
                quoted_storage_cfg = shell.quote(struct(**storage).to_json()),
            )),
        ),
    )

# Requires /<RPM_SNAPSHOT_BASE_DIR>/` to already have been created.
def install_rpm_repo_snapshot(snapshot, make_default = True):
    base_dir = paths.join("/", RPM_SNAPSHOT_BASE_DIR)
    dest_dir = paths.join(base_dir, mangle_target(snapshot))
    features = [
        image.install(snapshot, dest_dir),
        # See "Hack alert" above.
        image.install_buck_runnable(
            "//fs_image/rpm:yum-from-snapshot",
            paths.join(dest_dir, "yum-from-snapshot"),
        ),
    ]
    if make_default:
        features.append(
            image.symlink_dir(dest_dir, paths.join(base_dir, "default")),
        )
    return image.feature(features = features)
