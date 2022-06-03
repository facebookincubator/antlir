# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

# Artifacts that come from upstream kernel systems (aka, the rpms)
upstream_kernel_targets_t = shape.shape(
    main_rpm = target_t,
    devel_rpm = shape.field(target_t, optional = True),
    headers_rpm = shape.field(target_t, optional = True),
)

# Artifacts derived from the upstream kernel rpms
derived_kernel_targets_t = shape.shape(
    vmlinuz = target_t,
    modules_directory = target_t,
    disk_boot_modules = target_t,
    image = target_t,
)

kernel_t = shape.shape(
    uname = str,
    upstream_targets = shape.field(upstream_kernel_targets_t),
    derived_targets = shape.field(derived_kernel_targets_t),
)
