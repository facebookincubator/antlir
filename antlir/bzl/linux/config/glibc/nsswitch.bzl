# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl:shape.bzl", "shape")

_action = shape.shape(
    status = shape.enum("success", "notfound", "unavail", "tryagain"),
    action = shape.enum("return", "continue", "merge"),
)

_service = shape.shape(
    name = str,
    action = shape.field(_action, optional = True),
)

_database = shape.shape(
    # not an exhaustive list, but does contain the things we care about
    name = shape.enum("group", "hosts", "passwd", "shadow"),
    services = shape.list(_service),
)

_conf = shape.shape(
    databases = shape.list(_database),
)

def _new(**kwargs):
    return shape.new(_conf, **kwargs)

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
        instance = shape.new(_conf, **kwargs)

    file = _render(
        name = name,
        instance = instance,
    )
    return image.install(
        file,
        "/etc/nsswitch.conf",
    )

# exported api to instantiate an nsswitch config
nsswitch = struct(
    t = _conf,
    new = _new,
    install = _install,
    default = shape.new(
        _conf,
        databases = [
            shape.new(
                _database,
                name = "passwd",
                services = [
                    shape.new(_service, name = "files"),
                    shape.new(_service, name = "systemd"),
                ],
            ),
            shape.new(
                _database,
                name = "group",
                services = [
                    shape.new(
                        _service,
                        name = "files",
                        action = shape.new(
                            _action,
                            status = "success",
                            action = "merge",
                        ),
                    ),
                    shape.new(_service, name = "systemd"),
                ],
            ),
            shape.new(
                _database,
                name = "shadow",
                services = [
                    shape.new(_service, name = "files"),
                ],
            ),
            shape.new(
                _database,
                name = "hosts",
                services = [
                    shape.new(_service, name = "files"),
                    shape.new(_service, name = "dns"),
                ],
            ),
        ],
    ),
)
