# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load(":nsswitch.shape.bzl", "action_t", "conf_t", "database_t", "service_t")

def _new(**kwargs):
    return shape.new(conf_t, **kwargs)

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
        instance = shape.new(conf_t, **kwargs)

    file = _render(
        name = name,
        instance = instance,
    )
    return feature.install(
        file,
        "/etc/nsswitch.conf",
    )

# exported api to instantiate an nsswitch config
nsswitch = struct(
    t = conf_t,
    new = _new,
    install = _install,
    default = shape.new(
        conf_t,
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
