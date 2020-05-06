"""
Given a source tree and a matching RPM spec file, runs `rpmbuild` inside the
given image layer, and outputs a new layer with the resulting RPM(s)
available in a pre-determined location: `/rpmbuild/RPMS`.
"""

load("//fs_image/bzl:constants.bzl", "DEFAULT_BUILD_APPLIANCE")
load("//fs_image/bzl:image_foreign_layer.bzl", "image_foreign_layer")
load("//fs_image/bzl:image_layer.bzl", "image_layer")
load("//fs_image/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//fs_image/bzl:oss_shim.bzl", "buck_genrule")
load("//fs_image/bzl/image_actions:install.bzl", "image_install")
load("//fs_image/bzl/image_actions:mkdir.bzl", "image_mkdir")
load("//fs_image/bzl/image_actions:rpms.bzl", "image_rpms_install")
load("//fs_image/bzl/image_actions:tarball.bzl", "image_tarball")

def image_rpmbuild_layer(
        name,
        # The name of a specfile target (i.e. a single file made accessible
        # with `export_file()`).
        specfile,
        # The name of the target that has the source files for the RPM
        # (i.e. made accessible with `export_file()`).
        # Be aware that depending on the buck rule, the sources may not
        # have the intended directory structure (i.e. may or may not include
        # the top directory) when installed in the layer. You can look at the
        # test TARGETS for "toy-rpm" to see how the sources target is set up
        # so that there is no "toy_srcs" top directory.
        # It can be helpful to do
        # `buck run //path/to:<name>-rpmbuild-setup-container` to inspect the
        # setup layer and experiment with building.
        source,
        # Which RPM snapshots to use when installing build dependencies.
        serve_rpm_snapshots = (),
        # An `image.layer` target, on top of which the current layer will
        # build the RPM.  This should have `rpm-build`, optionally macro
        # packages like `redhat-rpm-config`, and any of the spec file's
        # build dependencies installed.
        parent_layer = DEFAULT_BUILD_APPLIANCE,
        **image_layer_kwargs):
    # Future: We tar the source directory and untar it inside the subvolume
    # before building because the "install_*_trees" feature does not yet
    # exist.
    source_tarball = name + "-source.tgz"
    buck_genrule(
        name = source_tarball,
        out = source_tarball,
        bash = '''
            tar --sort=name --mtime=2018-01-01 --owner=0 --group=0 \
                --numeric-owner -C $(location {source}) -czf "$OUT" .
        '''.format(source = maybe_export_file(source)),
        visibility = [],
    )

    specfile_path = "/rpmbuild/SPECS/specfile.spec"

    setup_layer = name + "-rpmbuild-setup"
    image_layer(
        name = setup_layer,
        parent_layer = parent_layer,
        features = [
            image_install(specfile, specfile_path),
            image_mkdir("/", "rpmbuild"),
            image_mkdir("/rpmbuild", "BUILD"),
            image_mkdir("/rpmbuild", "BUILDROOT"),
            image_mkdir("/rpmbuild", "RPMS"),
            image_mkdir("/rpmbuild", "SOURCES"),
            image_mkdir("/rpmbuild", "SPECS"),
            image_tarball(":" + source_tarball, "/rpmbuild/SOURCES"),
            # Needed to install RPM dependencies below
            image_rpms_install(["yum-utils"]),
        ],
        visibility = [],
    )

    rpmbuild_dir = "/rpmbuild"

    install_deps_layer = name + "-rpmbuild-install-deps"
    image_foreign_layer(
        name = install_deps_layer,
        rule_type = "image_rpmbuild_install_deps_layer",
        parent_layer = ":" + setup_layer,
        # Auto-installing RPM dependencies requires `root`.
        user = "root",
        cmd = [
            "yum-builddep",
            # Define the build directory for this project
            "--define=_topdir {}".format(rpmbuild_dir),
            # Future: Try removing the `--config` argument when yum is fully
            # transparent.
            "--config",
            "/__fs_image__/rpm/default-snapshot-for-installer/yum/yum/etc/yum/yum.conf",
            "--assumeyes",
            specfile_path,
        ],
        serve_rpm_snapshots = serve_rpm_snapshots,
        **image_layer_kwargs
    )

    image_foreign_layer(
        name = name,
        rule_type = "image_rpmbuild_layer",
        parent_layer = ":" + install_deps_layer,
        # While it's possible to want to support unprivileged builds, the
        # typical case will want to auto-install dependencies, which
        # requires `root`.
        user = "root",
        cmd = [
            "rpmbuild",
            # Change the destination for the built RPMs
            "--define=_topdir {}".format(rpmbuild_dir),
            # Don't include the version in the resulting RPM filenames
            "--define=_rpmfilename %%{NAME}.rpm",
            "-bb",  # Only build the binary packages (no SRPMs)
            "{}/SPECS/specfile.spec".format(rpmbuild_dir),
        ],
        **image_layer_kwargs
    )
