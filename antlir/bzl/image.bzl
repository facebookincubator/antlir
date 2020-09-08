"This provides a more friendly UI to the image_* macros."

load("//antlir/bzl/image_actions:clone.bzl", "image_clone")
load("//antlir/bzl/image_actions:feature.bzl", "image_feature")
load("//antlir/bzl/image_actions:install.bzl", "image_install", "image_install_buck_runnable")
load("//antlir/bzl/image_actions:mkdir.bzl", "image_mkdir")
load("//antlir/bzl/image_actions:mount.bzl", "image_host_dir_mount", "image_host_file_mount", "image_layer_mount")
load("//antlir/bzl/image_actions:remove.bzl", "image_remove")
load("//antlir/bzl/image_actions:rpms.bzl", "image_rpms_install", "image_rpms_remove_if_exists")
load("//antlir/bzl/image_actions:symlink.bzl", "image_symlink_dir", "image_symlink_file")
load("//antlir/bzl/image_actions:tarball.bzl", "image_tarball")
load(":image_cpp_unittest.bzl", "image_cpp_unittest")
load(":image_kernel_opts.bzl", "image_kernel_opts")
load(":image_layer.bzl", "image_layer")
load(":image_layer_alias.bzl", "image_layer_alias")
load(":image_package.bzl", "image_package")
load(":image_python_unittest.bzl", "image_python_unittest")
load(":image_sendstream_layer.bzl", "image_sendstream_layer")
load(":image_source.bzl", "image_source")

image = struct(
    cpp_unittest = image_cpp_unittest,
    clone = image_clone,
    feature = image_feature,
    mkdir = image_mkdir,
    install = image_install,
    install_buck_runnable = image_install_buck_runnable,
    tarball = image_tarball,
    remove = image_remove,
    rpms_install = image_rpms_install,
    rpms_remove_if_exists = image_rpms_remove_if_exists,
    symlink_dir = image_symlink_dir,
    symlink_file = image_symlink_file,
    host_dir_mount = image_host_dir_mount,
    host_file_mount = image_host_file_mount,
    layer_mount = image_layer_mount,
    layer = image_layer,
    layer_alias = image_layer_alias,
    opts = struct,
    kernel_opts = image_kernel_opts,
    package = image_package,
    python_unittest = image_python_unittest,
    sendstream_layer = image_sendstream_layer,
    source = image_source,
)
