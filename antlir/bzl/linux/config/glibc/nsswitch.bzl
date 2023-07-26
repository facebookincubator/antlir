# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", antlir2_feature = "feature")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:defs.bzl", antlir1_feature = "feature")
load(":nsswitch.shape.bzl", "action_t", "conf_t", "database_t", "service_t")

def _new(**kwargs):
    return conf_t(**kwargs)

def _render(name, instance):
    return shape.render_template(
        name = name,
        instance = instance,
        template = "//antlir/bzl/linux/config/glibc:nsswitch",
    )

def _install(instance = None, use_antlir2 = False, **kwargs):
    contents_hash = sha256_b64(str(kwargs))
    name = "nsswitch.conf--" + contents_hash

    if not instance:
        instance = conf_t(**kwargs)

    file = _render(
        name = name,
        instance = instance,
    )
    if use_antlir2:
        return antlir2_feature.install(
            src = file,
            dst = "/etc/nsswitch.conf",
        )

    # the rest of this function is Antlir1 code
    return antlir1_feature.install(
        file,
        "/etc/nsswitch.conf",
    )

# exported api to instantiate an nsswitch config
nsswitch = struct(
    t = conf_t,
    new = _new,
    install = _install,
    default = conf_t(
        databases = [
            shape.new(
                database_t,
                name = "passwd",
                services = [
                    shape.new(service_t, name = "files"),
                    shape.new(service_t, name = "systemd"),
                ],
            ),
            shape.new(
                database_t,
                name = "group",
                services = [
                    shape.new(
                        service_t,
                        name = "files",
                        action = shape.new(
                            action_t,
                            status = "success",
                            action = "merge",
                        ),
                    ),
                    shape.new(service_t, name = "systemd"),
                ],
            ),
            shape.new(
                database_t,
                name = "shadow",
                services = [
                    shape.new(service_t, name = "files"),
                ],
            ),
            shape.new(
                database_t,
                name = "hosts",
                services = [
                    shape.new(service_t, name = "files"),
                    shape.new(service_t, name = "dns"),
                ],
            ),
        ],
    ),
)
