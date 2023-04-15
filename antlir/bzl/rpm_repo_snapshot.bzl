# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:collections.bzl", "collections")
load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:shell.bzl", "shell")
load("@fbsource//tools/build_defs:lazy.bzl", "lazy")
load("//antlir/bzl/genrule/yum_dnf_cache:yum_dnf_cache.bzl", "image_yum_dnf_make_snapshot_cache")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load(":build_defs.bzl", "buck_genrule", "get_visibility")
load(":image_layer.bzl", "image_layer")
load(":maybe_export_file.bzl", "maybe_export_file")
load(":snapshot_install_dir.bzl", "RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR", "RPM_SNAPSHOT_BASE_DIR", "snapshot_install_dir")
load(":structs.bzl", "structs")
load(":wrap_runtime_deps.bzl", "maybe_wrap_executable_target")

_FB_INTERNAL_STORAGE = "manifold"

def _yum_or_dnf_wrapper(yum_or_dnf, snapshot_name):
    if yum_or_dnf not in ("yum", "dnf"):
        fail("{} must be `yum` or `dnf`".format(yum_or_dnf))
    name = "{}-for-snapshot--{}".format(yum_or_dnf, snapshot_name)
    buck_genrule(
        name = name,
        bash = 'echo {} > "$OUT" && chmod u+rx "$OUT"'.format(shell.quote(
            """\
#!/bin/sh
set -ue -o pipefail -o noclobber
my_path=\\$(readlink -f "$0")
# If executed straight out of the snapshot, let `yum-dnf-from-snapshot`
# figure out what binary it is wrapping.
if [[ "$my_path" == /__antlir__/* ]] ; then
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
        labels = ["uses_sudo"],
        visibility = ["PUBLIC"],
    )
    return ":" + name

def rpm_repo_snapshot(
        name,
        src,
        storage,
        rpm_installers,
        repo_server_ports = None,
        visibility = None):
    '''
    Takes a bare in-repo snapshot, enriches it with `storage.sql3` from
    storage, and injects some auxiliary binaries & data.  This prepares the
    snapshot for installation into a build appliance (or
    `image.genrule_layer`) via `install_rpm_repo_snapshot`.

      - `storage`: JSON config for an `antlir.rpm.storage` class.

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
    if repo_server_ports == None:
        repo_server_ports = (28889, 28890)
    if not rpm_installers or not lazy.is_all(
        # CAREFUL: Below we assume that installer names need no shell-quoting.
        lambda p: (p in ["yum", "dnf"]),
        rpm_installers,
    ):
        fail(
            'Must contain >= 1 of "yum" / "dnf", got {}'.format(rpm_installers),
            "rpm_installers",
        )

    # For tests, we want relative `base_dir` to point into the snapshot dir.
    cli_storage = dict(storage)
    if cli_storage["kind"] == "filesystem" and \
       not cli_storage["base_dir"].startswith("/"):
        cli_storage["base_dir"] = "$(location {})/{}".format(
            src,
            cli_storage["base_dir"],
        )

    # We need a wrapper to `cp` a `buck run`nable target in @mode/dev.
    _, yum_dnf_from_snapshot_wrapper = maybe_wrap_executable_target(
        target = "//antlir/rpm:yum-dnf-from-snapshot",
        wrap_suffix = "rpm_repo_snapshot",
        visibility = [],
        runs_in_build_steps_causes_slow_rebuilds = True,  # Builds install RPMs
    )

    # Future: remove this in favor of linking it with `repo_servers.py`.
    # See the comment in that file for more info.
    _, repo_server_wrapper = maybe_wrap_executable_target(
        target = "//antlir/rpm{fb_dir}:repo-server".format(
            fb_dir = "/facebook" if cli_storage["kind"] == _FB_INTERNAL_STORAGE else "",
        ),
        wrap_suffix = "rpm_repo_snapshot",
        visibility = [],
        runs_in_build_steps_causes_slow_rebuilds = True,  # Builds install RPMs
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
$(exe_target //antlir/rpm:write-yum-dnf-conf) \\
    --rpm-installer {prog} \\
    --input-conf "$OUT"/snapshot/{prog}.conf \\
    --output-dir "$OUT"/{prog} \\
    --install-dir {quoted_install_dir}/{prog} \\
    --repo-server-ports {quoted_repo_server_ports} \\

mkdir -p "$OUT"/{prog}/bin
cp $(location {yum_or_dnf_wrapper}) "$OUT"/{prog}/bin/{prog}

# Fixme: remove this shim once we're actually calling `makecache` to make it.
mkdir -p "$OUT"/{prog}/var/cache/{prog}
'''.format(
            prog = rpm_installer,
            yum_or_dnf_wrapper = _yum_or_dnf_wrapper(rpm_installer, name),
            quoted_install_dir = shell.quote(snapshot_install_dir(":" + name)),
            quoted_repo_server_ports = quoted_repo_server_ports,
        ))

    buck_genrule(
        name = name,
        bash = '''\
set -ue -o pipefail -o noclobber

mkdir "$OUT"

# Copy the basic snapshot, e.g. `snapshot.storage_id`, `repos`, `yum|dnf.conf`
cp --no-target-directory -r $(location {src}) "$OUT/snapshot"

$(exe_target //antlir/rpm/storage{fb_dir}:cli) --storage {quoted_cli_storage_cfg} \\
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
            quoted_cli_storage_cfg = shell.quote(
                structs.as_json(struct(**cli_storage)),
            ),
            yum_dnf_from_snapshot_wrapper = yum_dnf_from_snapshot_wrapper,
            repo_server_wrapper = repo_server_wrapper,
            quoted_repo_server_ports = quoted_repo_server_ports,
            quoted_storage_cfg = shell.quote(
                structs.as_json(struct(**storage)),
            ),
            per_installer_cmds = "\n".join(per_installer_cmds),
            fb_dir = "/facebook" if cli_storage["kind"] == _FB_INTERNAL_STORAGE else "",
        ),
        # This rule is not cacheable due to `maybe_wrap_executable_target`
        # above.  Technically, we could make it cacheable in @mode/opt, but
        # since this is essentially a cache-retrieval rule already, that
        # doesn't seem useful.  For the same reason, nor does it seem that
        # useful to move the binary installation as `install_buck_runnable`
        # into `install_rpm_repo_snapshot`.
        cacheable = False,
        labels = ["uses_sudo"],
        visibility = get_visibility(visibility),
    )

def _set_up_rpm_repo_snapshots():
    # This will fail loudly if the two constants stop being siblings.
    defaults_dir = paths.relativize(
        RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR,
        paths.dirname(RPM_SNAPSHOT_BASE_DIR),
    )
    return [
        feature.ensure_subdirs_exist("/", RPM_SNAPSHOT_BASE_DIR),
        feature.ensure_subdirs_exist("/__antlir__/rpm", defaults_dir),
    ]

def install_rpm_repo_snapshot(snapshot):
    """
    Returns an `feature.new`, which installs the `rpm_repo_snapshot`
    target in `snapshot` in its canonical location.

    A layer that installs snapshots should be followed by a
    `image_yum_dnf_make_snapshot_cache` layer so that `yum` / `dnf` repodata
    caches are properly populated.  Otherwise, RPM installs will be slow.
    """

    snapshot_dir = snapshot_install_dir(snapshot)
    if snapshot.endswith(".layer"):
        features = [feature.clone(snapshot, "", snapshot_dir)]
    else:
        features = [feature.install(snapshot, snapshot_dir)]

    return _set_up_rpm_repo_snapshots() + features

def default_rpm_repo_snapshot_for(prog, snapshot):
    """
    Set the default snapshot for the given RPM installer.  The snapshot must
    have been installed by `install_rpm_repo_snapshot()`.
    """

    link_name = RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR + "/" + prog
    return [
        # Silently replace the parent's default because there's not an
        # obvious scenario in which this is an error, and so forcing the
        # user to pass an explicit `replace_existing` flag seems unhelpful.
        feature.remove(link_name, must_exist = False),
        feature.ensure_dir_symlink(snapshot_install_dir(snapshot), link_name),
    ]

def add_rpm_repo_snapshots_layer(
        name,
        parent_layer,
        dnf_snapshot = None,  # install, make default for `dnf`, make cache
        yum_snapshot = None,  # install, make default for `yum`, make cache
        extra_snapshot_installers = None,  # install, make cache
        make_caches = False,
        remove_existing_snapshot = False,
        **image_layer_kwargs):
    """
    For the specified snapshots, install them into the parent layer, and
    pre-generate the repodata caches for each of the mentioned snapshots and
    installers.

    This is meant to be the most common way of installing snapshots into
    layers, so it acts as syntax sugar.

    A careful reader will note that we could automatically build caches for
    all installers supported by a snapshot, but we currently do not do this
    because building caches is fairly slow, whereas a supported installer is
    not necessarily going to get used.
    """

    if remove_existing_snapshot:
        tmp_name = name + "__remove-existing-snapshot-" + name
        image_layer(
            name = tmp_name,
            parent_layer = parent_layer,
            features = [
                feature.remove(RPM_SNAPSHOT_BASE_DIR, must_exist = False),
            ],
            **image_layer_kwargs
        )
        parent_layer = ":" + tmp_name

    features = _set_up_rpm_repo_snapshots()
    default_s_i_pairs = [
        (s, i)
        for s, i in [(dnf_snapshot, "dnf"), (yum_snapshot, "yum")]
        if s != None
    ]
    for snapshot, installer in default_s_i_pairs:
        features.append(default_rpm_repo_snapshot_for(installer, snapshot))

    make_cache_for_s_i_pairs = {}
    for s_i in (
        default_s_i_pairs + (extra_snapshot_installers or [])
    ):
        if s_i not in make_cache_for_s_i_pairs:
            make_cache_for_s_i_pairs[s_i] = True
            features.append(install_rpm_repo_snapshot(s_i[0]))

    name_without_caches = name + "__precursor-without-caches-to-" + name
    image_layer(
        name = name_without_caches if make_caches else name,
        parent_layer = parent_layer,
        features = features,
        **image_layer_kwargs
    )

    if not make_caches:
        return

    snapshot_to_installers = {}
    for snapshot, installer in make_cache_for_s_i_pairs.keys():
        snapshot_to_installers.setdefault(snapshot, []).append(installer)

    image_yum_dnf_make_snapshot_cache(
        name = name,
        parent_layer = ":" + name_without_caches,
        snapshot_to_installers = snapshot_to_installers,
        **image_layer_kwargs
    )
