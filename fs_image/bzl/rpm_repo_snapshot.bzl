load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:shell.bzl", "shell")
load(":image.bzl", "image")
load(":maybe_export_file.bzl", "maybe_export_file")
load(":oss_shim.bzl", "buck_genrule")
load(":target_tagger.bzl", "mangle_target", "maybe_wrap_executable_target")

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

    # We need a wrapper to `cp` a `buck run`nable target in @mode/dev.
    _, yum_from_snapshot_wrapper = maybe_wrap_executable_target(
        target = "//fs_image/rpm:yum-from-snapshot",
        wrap_prefix = "__rpm_repo_snapshot",
        visibility = [],
    )
    buck_genrule(
        name = name,
        out = "unused",
        bash = '''\
set -ue -o pipefail -o noclobber

# Rebuild RPM snapshots if this bzl (or its dependencies) change
echo $(location //fs_image/bzl:rpm_repo_snapshot) > /dev/null

# Copy the basic snapshot, e.g. `snapshot.storage_id`, `repos`, `yum.conf`
cp --no-target-directory -r $(location {src}) "$OUT"

$(exe //fs_image/rpm/storage:cli) --storage {quoted_cli_storage_cfg} \
    get "\\$(cat "$OUT"/snapshot.storage_id)" > "$OUT"/snapshot.sql3
# Write-protect the SQLite DB because it's common to use the `sqlite3` to
# inspect it, and it's otherwise very easy to accidentally mutate the build
# artifact, which is a big no-no.
chmod a-w "$OUT"/snapshot.sql3

# This lets `nspawn-in-subvol` access the storage config, and not just `yum`
echo {quoted_storage_cfg} > "$OUT"/storage.json

cp $(location {yum_from_snapshot_wrapper}) "$OUT"/yum-from-snapshot

# The `bin` directory exists so that "porcelain" binaries can potentially be
# added to `PATH`.  But we should avoid doing this in production code.
mkdir "$OUT"/bin

cp $(location {yum_sh_target}) "$OUT"/bin/yum
        '''.format(
            src = maybe_export_file(src),
            quoted_cli_storage_cfg = shell.quote(struct(**cli_storage).to_json()),
            quoted_storage_cfg = shell.quote(struct(**storage).to_json()),
            yum_sh_target = "//fs_image/bzl:files/yum.sh",
            yum_from_snapshot_wrapper = yum_from_snapshot_wrapper,
        ),
    )

# Requires some other feature to make the directory `/<RPM_SNAPSHOT_BASE_DIR>`
def install_rpm_repo_snapshot(snapshot, make_default = True):
    base_dir = paths.join("/", RPM_SNAPSHOT_BASE_DIR)
    dest_dir = paths.join(base_dir, mangle_target(snapshot))
    features = [image.install(snapshot, dest_dir)]
    if make_default:
        features.append(
            image.symlink_dir(dest_dir, paths.join(base_dir, "default")),
        )
    return image.feature(features = features)
