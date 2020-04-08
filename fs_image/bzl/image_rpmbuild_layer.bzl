load("//fs_image/bzl/image_actions:install.bzl", "image_install")
load("//fs_image/bzl/image_actions:mkdir.bzl", "image_mkdir")
load("//fs_image/bzl/image_actions:tarball.bzl", "image_tarball")
load(":compile_image_features.bzl", "compile_image_features")
load(":constants.bzl", "BUILD_APPLIANCE_TARGET")
load(":image_layer.bzl", "image_layer")
load(":image_layer_utils.bzl", "image_layer_utils")
load(":image_utils.bzl", "image_utils")
load(":maybe_export_file.bzl", "maybe_export_file")
load(":oss_shim.bzl", "buck_genrule")

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
        # An `image.layer` target, on top of which the current layer will
        # build the RPM.  This should have `rpm-build`, optionally macro
        # packages like `redhat-rpm-config`, and any of the spec file's
        # build dependencies installed.
        parent_layer = BUILD_APPLIANCE_TARGET,
        **image_layer_kwargs):
    if "features" in image_layer_kwargs:
        fail("\"features\" are not supported in image_rpmbuild_layer")

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
    )

    setup_layer = name + "-rpmbuild-setup"
    image_layer(
        name = setup_layer,
        parent_layer = parent_layer,
        features = [
            image_install(specfile, "/rpmbuild/SPECS/specfile.spec"),
            image_mkdir("/", "rpmbuild"),
            image_mkdir("/rpmbuild", "BUILD"),
            image_mkdir("/rpmbuild", "BUILDROOT"),
            image_mkdir("/rpmbuild", "RPMS"),
            image_mkdir("/rpmbuild", "SOURCES"),
            image_mkdir("/rpmbuild", "SPECS"),
            image_tarball(":" + source_tarball, "/rpmbuild/SOURCES"),
        ],
        **image_layer_kwargs
    )

    image_layer_utils.image_layer_impl(
        _rule_type = "image_rpmbuild_layer",
        _layer_name = name,
        _make_subvol_cmd = compile_image_features(
            current_target = image_utils.current_target(name),
            parent_layer = ":" + setup_layer,
            features = [struct(
                items = struct(
                    rpm_build = [{"rpmbuild_dir": "/rpmbuild"}],
                ),
                deps = [],
            )],
            build_opts = None,
        ),
    )
