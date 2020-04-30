load("@bazel_skylib//lib:collections.bzl", "collections")
load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:shell.bzl", "shell")
load("//fs_image/bzl/image_actions:feature.bzl", "image_feature")
load("//fs_image/bzl/image_actions:install.bzl", "image_install")
load("//fs_image/bzl/image_actions:mkdir.bzl", "image_mkdir")
load("//fs_image/bzl/image_actions:remove.bzl", "image_remove")
load("//fs_image/bzl/image_actions:symlink.bzl", "image_symlink_dir")
load(":maybe_export_file.bzl", "maybe_export_file")
load(":oss_shim.bzl", "buck_genrule", "get_visibility")
load(":target_tagger.bzl", "mangle_target", "maybe_wrap_executable_target")

# KEEP IN SYNC with its copy in `rpm/find_snapshot.py`
RPM_SNAPSHOT_BASE_DIR = "__fs_image__/rpm/repo-snapshot"

# KEEP IN SYNC with its copy in `rpm/find_snapshot.py`
def snapshot_install_dir(snapshot):
    return paths.join("/", RPM_SNAPSHOT_BASE_DIR, mangle_target(snapshot))

def yum_or_dnf_wrapper(name):
    if name not in ("yum", "dnf"):
        fail("Must be `yum` or `dnf`", "yum_or_dnf")
    buck_genrule(
        name = name,
        out = "ignored",
        bash = 'echo {} > "$OUT" && chmod u+rx "$OUT"'.format(shell.quote(
            """\
#!/bin/sh
set -ue -o pipefail -o noclobber
my_path=\\$(readlink -f "$0")
my_dir=\\$(dirname "$my_path")
base_dir=\\$(dirname "$my_dir")
exec "$base_dir"/yum-dnf-from-snapshot \\
    --snapshot-dir "$base_dir" \\
    {yum_or_dnf} -- "$@"
""".format(yum_or_dnf = shell.quote(name)),
        )),
    )

def rpm_repo_snapshot(
        name,
        src,
        storage,
        rpm_installers,
        repo_server_ports = (28889, 28890),
        visibility = None):
    '''
    Takes a bare in-repo snapshot, enriches it with `storage.sql3` from
    storage, and injects some auxiliary binaries & data.  This prepares the
    snapshot for installation into a build appliance (or
    `image_foreign_layer`) via `install_rpm_repo_snapshot`.

      - `storage`: JSON config for an `fs_image.rpm.storage` class.

        Hack alert: If you pass `storage["kind"] == "filesystem"`, and its
        `"base_dir"` is relative, it will magically be interpreted to be
        relative to the snapshot dir, both by this code, and later by
        `yum-dnf-from-snapshot` that runs in the build appliance.

      - `rpm_installers`: A tuple of 'yum', or 'dnf', or both.  The
         snapshotted repos might be compatible with (or tested with) only
         one package manager but not with the other.

      - `repo_server_ports`: Hardcodes into the snapshot some localhost
        ports, on which the RPM repo snapshots will be served.  Hardcoding
        is required because the server URI is part of the cache key for
        `dnf`.  Moreover, hardcoded ports make container setup easier.

        The main impact of this port list is that its **size** determines
        the maximum number of repo objects (e.g.  RPMs) that `yum` / `dnf`
        can concurrently fetch at installation time.

        The defaults were picked at random from [16384, 32768) to avoid the
        default ephemeral port range of 32768+, and to avoid lower port #s
        which tend to be reserved for services.

        We should not have issues with port collisions, because nothing else
        should be running in the container when we install RPMs.  If you do
        have a collision (e.g. due to installing multiple snapshots), they
        are easy enough to change.

        Future: If collisions due to multi-snapshot installations are
        commonplace, we could generate the ports via a deterministic hash of
        the normalized target name (akin to `mangle_target`), so that they
        wouldn't avoid collision by default.
    '''
    if not rpm_installers or not all([
        (p in ["yum", "dnf"])
        for p in rpm_installers
    ]):
        fail(
            'Must contain >= 1 of "yum" / "dnf", got {}'.format(rpm_installers),
            "rpm_installers",
        )

    # For tests, we want relative `base_dir` to point into the snapshot dir.
    cli_storage = storage.copy()
    if cli_storage["kind"] == "filesystem" and \
       not cli_storage["base_dir"].startswith("/"):
        cli_storage["base_dir"] = "$(location {})/{}".format(
            src,
            cli_storage["base_dir"],
        )

    # We need a wrapper to `cp` a `buck run`nable target in @mode/dev.
    _, yum_dnf_from_snapshot_wrapper = maybe_wrap_executable_target(
        target = "//fs_image/rpm:yum-dnf-from-snapshot",
        wrap_prefix = "__rpm_repo_snapshot",
        visibility = [],
    )

    # Future: remove this in favor of linking it with `inject_repo_servers.py`.
    # See the comment in that file for more info.
    _, repo_server_wrapper = maybe_wrap_executable_target(
        target = "//fs_image/rpm:repo-server",
        wrap_prefix = "__rpm_repo_snapshot",
        visibility = [],
    )
    quoted_repo_server_ports = shell.quote(
        " ".join([
            str(p)
            for p in sorted(collections.uniq(repo_server_ports))
        ]),
    )
    buck_genrule(
        name = name,
        out = "ignored",
        bash = '''\
set -ue -o pipefail -o noclobber

# Rebuild RPM snapshots if this bzl (or its dependencies) change
echo $(location //fs_image/bzl:rpm_repo_snapshot) > /dev/null

# Copy the basic snapshot, e.g. `snapshot.storage_id`, `repos`, `yum|dnf.conf`
cp --no-target-directory -r $(location {src}) "$OUT"

$(exe //fs_image/rpm/storage:cli) --storage {quoted_cli_storage_cfg} \
    get "\\$(cat "$OUT"/snapshot.storage_id)" > "$OUT"/snapshot.sql3
# Write-protect the SQLite DB because it's common to use the `sqlite3` to
# inspect it, and it's otherwise very easy to accidentally mutate the build
# artifact, which is a big no-no.
chmod a-w "$OUT"/snapshot.sql3

# Many programs access the storage config: `yum`, `dnf`, `nspawn-in-subvol`
echo {quoted_storage_cfg} > "$OUT"/storage.json

cp $(location {yum_dnf_from_snapshot_wrapper}) "$OUT"/yum-dnf-from-snapshot
cp $(location {repo_server_wrapper}) "$OUT"/repo-server
# It's possible but harder to parse these from a rewritten `yum|dnf.conf`.
echo {quoted_repo_server_ports} > "$OUT"/repo_server_ports

# Save rewritten `yum|dnf.conf` files that are ready to serve the snapshot.
$(exe //fs_image/rpm:write-yum-dnf-conf) \\
    --output-dir "$OUT"/etc \\
    --install-dir {quoted_install_dir}/etc \\
    {quoted_write_conf_args}

# The `bin` directory exists so that "porcelain" binaries can potentially be
# added to `PATH`.  But we should avoid doing this in production code.
mkdir "$OUT"/bin
{maybe_add_bin_dnf}
{maybe_add_bin_yum}
        '''.format(
            src = maybe_export_file(src),
            quoted_cli_storage_cfg = shell.quote(struct(**cli_storage).to_json()),
            quoted_storage_cfg = shell.quote(struct(**storage).to_json()),
            yum_dnf_from_snapshot_wrapper = yum_dnf_from_snapshot_wrapper,
            repo_server_wrapper = repo_server_wrapper,
            quoted_repo_server_ports = quoted_repo_server_ports,
            quoted_install_dir = shell.quote(snapshot_install_dir(":" + name)),
            quoted_write_conf_args = " ".join([
                '--write-conf {ri} "$OUT"/{ri}.conf {p}'.format(
                    ri = shell.quote(rpm_installer),
                    p = quoted_repo_server_ports,
                )
                for rpm_installer in rpm_installers
            ]),
            # Only install binaries this snapshot claims to support.
            maybe_add_bin_dnf = (
                'cp $(location //fs_image/bzl:dnf) "$OUT"/bin/dnf'
            ) if "dnf" in rpm_installers else "",
            maybe_add_bin_yum = (
                'cp $(location //fs_image/bzl:yum) "$OUT"/bin/yum'
            ) if "yum" in rpm_installers else "",
        ),
        # This rule is not cacheable due to `maybe_wrap_executable_target`
        # above.  Technically, we could make it cacheable in @mode/opt, but
        # since this is essentially a cache-retrieval rule already, that
        # doesn't seem useful.  For the same reason, nor does it seem that
        # useful to move the binary installation as `install_buck_runnable`
        # into `install_rpm_repo_snapshot`.
        cacheable = False,
        visibility = get_visibility(visibility, name),
    )

# Future: Once we have `ensure_dir_exists`, this can be implicit.
def set_up_rpm_repo_snapshots():
    return image_feature(features = [
        image_mkdir("/", RPM_SNAPSHOT_BASE_DIR),
        image_mkdir("/__fs_image__/rpm", "default-snapshot-for-installer"),
    ])

def install_rpm_repo_snapshot(snapshot):
    """
    Returns an `image.feature`, which installs the `rpm_repo_snapshot`
    target in `snapshot` in its canonical location.

    The layer must also include `set_up_rpm_repo_snapshots()`.
    """
    dest_dir = snapshot_install_dir(snapshot)
    features = [image_install(snapshot, dest_dir)]
    return image_feature(features = features)

def default_rpm_repo_snapshot_for(prog, snapshot):
    """
    Set the default snapshot for the given RPM installer.  The snapshot must
    have been installed by `install_rpm_repo_snapshot()`.
    """

    # Keep in sync with `rpm_action.py` and `set_up_rpm_repo_snapshots()`
    link_name = "__fs_image__/rpm/default-snapshot-for-installer/" + prog
    return image_feature(features = [
        # Silently replace the parent's default because there's not an
        # obvious scenario in which this is an error, and so forcing the
        # user to pass an explicit `replace_existing` flag seems unhelpful.
        image_remove(link_name, must_exist = False),
        image_symlink_dir(snapshot_install_dir(snapshot), link_name),
    ])
