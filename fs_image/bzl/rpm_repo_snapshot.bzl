load("@bazel_skylib//lib:collections.bzl", "collections")
load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:shell.bzl", "shell")
load("//fs_image/bzl/image_actions:install.bzl", "image_install")
load("//fs_image/bzl/image_actions:mkdir.bzl", "image_mkdir")
load("//fs_image/bzl/image_actions:remove.bzl", "image_remove")
load("//fs_image/bzl/image_actions:symlink.bzl", "image_symlink_dir")
load(":maybe_export_file.bzl", "maybe_export_file")
load(":oss_shim.bzl", "buck_genrule", "get_visibility")
load(":target_helpers.bzl", "mangle_target")
load(":wrap_runtime_deps.bzl", "maybe_wrap_executable_target")

# KEEP IN SYNC with its copy in `rpm/find_snapshot.py`
RPM_SNAPSHOT_BASE_DIR = "__fs_image__/rpm/repo-snapshot"

# KEEP IN SYNC with its copy in `rpm/find_snapshot.py`
def snapshot_install_dir(snapshot):
    return paths.join("/", RPM_SNAPSHOT_BASE_DIR, mangle_target(snapshot))

def _yum_or_dnf_wrapper(yum_or_dnf, snapshot_name):
    if yum_or_dnf not in ("yum", "dnf"):
        fail("{} must be `yum` or `dnf`".format(yum_or_dnf))
    name = "{}-for-snapshot--{}".format(yum_or_dnf, snapshot_name)
    buck_genrule(
        name = name,
        out = "ignored",
        bash = 'echo {} > "$OUT" && chmod u+rx "$OUT"'.format(shell.quote(
            """\
#!/bin/sh
set -ue -o pipefail -o noclobber
my_path=\\$(readlink -f "$0")
# If executed straight out of the snapshot, let `yum-dnf-from-snapshot`
# figure out what binary it is wrapping.
if [[ "$my_path" == /__fs_image__/* ]] ; then
    yum_dnf_binary_args=()
else
    yum_dnf_binary_args=(--yum-dnf-binary "$my_path")
fi
exec -a "$0" {quoted_snap_dir}/yum-dnf-from-snapshot \\
    --snapshot-dir {quoted_snap_dir} \\
    "${{yum_dnf_binary_args[@]}}" \\
    -- {yum_or_dnf} -- "$@"
""".format(
                quoted_snap_dir = shell.quote(snapshot_install_dir(":" + snapshot_name)),
                yum_or_dnf = shell.quote(yum_or_dnf),
            ),
        )),
        visibility = ["PUBLIC"],
    )
    return ":" + name

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
        # CAREFUL: Below we assume that installer names need no shell-quoting.
        (p in ["yum", "dnf"])
        for p in rpm_installers
    ]):
        fail(
            'Must contain >= 1 of "yum" / "dnf", got {}'.format(rpm_installers),
            "rpm_installers",
        )

    # For tests, we want relative `base_dir` to point into the snapshot dir.
    cli_storage = dict(**storage)
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

    # Future: remove this in favor of linking it with `repo_servers.py`.
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

    # For each RPM installer supported by the snapshot, install:
    #   - A rewritten config that is ready to serve the snapshot (assuming
    #     that its repo-servers are started).  `yum-dnf-from-snapshot` knows
    #     to search the location we set in `--install-dir`.
    #   - A transparent wrapper that runs the RPM installer with extra
    #     sandboxing, and using our rewritten config.  Each binary goes in
    #     `bin` subdir, making it easy to add any one binary to `PATH`.
    per_installer_cmds = []
    for rpm_installer in rpm_installers:
        per_installer_cmds.append('''
mkdir -p "$OUT"/{prog}/bin
cp $(location {yum_or_dnf_wrapper}) "$OUT"/{prog}/bin/{prog}

$(exe //fs_image/rpm:write-yum-dnf-conf) \\
    --rpm-installer {prog} \\
    --input-conf "$OUT"/snapshot/{prog}.conf \\
    --output-dir "$OUT"/{prog}/etc \\
    --install-dir {quoted_install_dir}/{prog}/etc \\
    --repo-server-ports {quoted_repo_server_ports} \\

'''.format(
            prog = rpm_installer,
            yum_or_dnf_wrapper = _yum_or_dnf_wrapper(rpm_installer, name),
            quoted_install_dir = shell.quote(snapshot_install_dir(":" + name)),
            quoted_repo_server_ports = quoted_repo_server_ports,
        ))

    buck_genrule(
        name = name,
        out = "ignored",
        bash = '''\
set -ue -o pipefail -o noclobber

# Make sure FB CI considers RPM snapshots changed if this bzl (or its
# dependencies) change.
echo $(location //fs_image/bzl:rpm_repo_snapshot) > /dev/null

mkdir "$OUT"

# Copy the basic snapshot, e.g. `snapshot.storage_id`, `repos`, `yum|dnf.conf`
cp --no-target-directory -r $(location {src}) "$OUT/snapshot"

$(exe //fs_image/rpm/storage:cli) --storage {quoted_cli_storage_cfg} \\
    get "\\$(cat "$OUT"/snapshot/snapshot.storage_id)" \\
        > "$OUT"/snapshot/snapshot.sql3
# Write-protect the SQLite DB because it's common to use the `sqlite3` to
# inspect it, and it's otherwise very easy to accidentally mutate the build
# artifact, which is a big no-no.
chmod a-w "$OUT"/snapshot/snapshot.sql3

cp $(location {yum_dnf_from_snapshot_wrapper}) "$OUT"/yum-dnf-from-snapshot

cp $(location {repo_server_wrapper}) "$OUT"/repo-server
# It's possible but harder to parse these from a rewritten `yum|dnf.conf`.
echo {quoted_repo_server_ports} > "$OUT"/ports-for-repo-server
# Tells `repo-server`s how to connect to the snapshot storage.
echo {quoted_storage_cfg} > "$OUT"/snapshot/storage.json

{per_installer_cmds}
        '''.format(
            src = maybe_export_file(src),
            quoted_cli_storage_cfg = shell.quote(struct(**cli_storage).to_json()),
            yum_dnf_from_snapshot_wrapper = yum_dnf_from_snapshot_wrapper,
            repo_server_wrapper = repo_server_wrapper,
            quoted_repo_server_ports = quoted_repo_server_ports,
            quoted_storage_cfg = shell.quote(struct(**storage).to_json()),
            per_installer_cmds = "\n".join(per_installer_cmds),
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
    return [
        image_mkdir("/", RPM_SNAPSHOT_BASE_DIR),
        image_mkdir("/__fs_image__/rpm", "default-snapshot-for-installer"),
    ]

def install_rpm_repo_snapshot(snapshot):
    """
    Returns an `image.feature`, which installs the `rpm_repo_snapshot`
    target in `snapshot` in its canonical location.

    The layer must also include `set_up_rpm_repo_snapshots()`.
    """
    return [image_install(snapshot, snapshot_install_dir(snapshot))]

def default_rpm_repo_snapshot_for(prog, snapshot):
    """
    Set the default snapshot for the given RPM installer.  The snapshot must
    have been installed by `install_rpm_repo_snapshot()`.
    """

    # Keep in sync with `rpm_action.py` and `set_up_rpm_repo_snapshots()`
    link_name = "__fs_image__/rpm/default-snapshot-for-installer/" + prog
    return [
        # Silently replace the parent's default because there's not an
        # obvious scenario in which this is an error, and so forcing the
        # user to pass an explicit `replace_existing` flag seems unhelpful.
        image_remove(link_name, must_exist = False),
        image_symlink_dir(snapshot_install_dir(snapshot), link_name),
    ]
