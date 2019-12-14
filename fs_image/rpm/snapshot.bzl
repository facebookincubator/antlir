load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:shell.bzl", "shell")
load("//fs_image/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//fs_image/bzl:oss_shim.bzl", "buck_genrule")

def rpm_repo_snapshot(name, src, storage, repo_subdir = None):
    buck_genrule(
        name = name,
        out = "unused",
        bash = '''\
        set -ue -o pipefail
        cp --no-target-directory -r $(location {src}) "$OUT"
        $(exe //fs_image/rpm/storage:cli) --storage {quoted_storage_cfg} \
            get "\$(cat "$OUT"/{quoted_repo_subdir}snapshot.storage_id)" \
                > "$OUT"/{quoted_repo_subdir}snapshot.sql3
        '''.format(
            src = maybe_export_file(src),
            quoted_storage_cfg = shell.quote(struct(**storage).to_json()),
            quoted_repo_subdir = (
                # This is helpful when the snapshot is a subdir of a target.
                shell.quote(paths.normalize(repo_subdir) + "/") if repo_subdir else ""
            ),
        ),
    )
