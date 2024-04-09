# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
load(":nsswitch.shape.bzl", "action_t", "conf_t", "database_t", "service_t")

def _new(**kwargs):
    return conf_t(**kwargs)

def _render(name, instance):
    return shape.render_template(
        name = name,
        instance = instance,
        template = "//antlir/bzl/linux/config/glibc:nsswitch",
    )

def _install(instance = None, **kwargs):
    contents_hash = sha256_b64(str(kwargs))
    name = "nsswitch.conf--" + contents_hash

    if not instance:
        instance = conf_t(**kwargs)

    file = _render(
        name = name,
        instance = instance,
    )
    return feature.install(
        src = file,
        dst = "/etc/nsswitch.conf",
    )

# exported api to instantiate an nsswitch config
nsswitch = struct(
    t = conf_t,
    new = _new,
    install = _install,
    default = conf_t(
        databases = [
            database_t(
                name = "passwd",
                services = [
                    service_t(name = "files"),
                    service_t(name = "systemd"),
                ],
            ),
            database_t(
                name = "group",
                services = [
                    service_t(
                        name = "files",
                        action = action_t(
                            status = "success",
                            action = "merge",
                        ),
                    ),
                    service_t(name = "systemd"),
                ],
            ),
            database_t(
                name = "shadow",
                services = [
                    service_t(name = "files"),
                ],
            ),
            database_t(
                name = "hosts",
                services = [
                    service_t(name = "files"),
                    service_t(name = "dns"),
                ],
            ),
        ],
    ),
)
