# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("@bazel_skylib//lib:types.bzl", "types")
load(":image.bzl", "image")
load(":oss_shim.bzl", "target_utils")
load(":shape.bzl", "shape")

PROVIDER_ROOT = "/usr/lib/systemd/system"
ADMIN_ROOT = "/etc/systemd/system"
TMPFILES_ROOT = "/etc/tmpfiles.d"

def _fail_if_path(thing, monkeymsg):
    """ If thing is a path do a big ole fail and prepend the monkey message for
        helping the human with some context in the fail message.
    """
    if "/" in thing:
        fail(monkeymsg + "({}) is a path, that is not allowed".format(thing))
    else:
        return thing

# Generate an image feature that masks the specified systemd units/configs
def _mask_impl(
        # List of things (i.e. full unit names or config names) to mask
        items,
        # The root directory of where each item's symlink will reside
        root,
        # Informational string that describes what is being masked. Prepended
        # to an error message on path verification failure.
        description):
    symlink_actions = []

    for item in items:
        _fail_if_path(item, description)

        symlink_actions.append(
            image.symlink_file(
                "/dev/null",
                paths.join(root, item),
            ),
        )

    return symlink_actions

def _mask_tmpfiles(
        # List of tmpfiles.d configs to disable. This should be in the full form
        # of the base name of the config, ie: dbus.conf, portables.conf, etc.
        configs):
    return _mask_impl(configs, TMPFILES_ROOT, "Mask tmpfiles.d config")

def _mask_units(
        # List of systemd units to mask (e.g. sshd.service). This should be in
        # the full form of the service, ie: unit.service, unit.mount,
        # unit.socket, etc..
        units):
    return _mask_impl(units, ADMIN_ROOT, "Mask Unit")

def _unmask_units(
        # list of systemd units to unmask (e.g. sshd.service). This should be in
        # the full form of the service, ie: unit.service, unit.mount,
        # unit.socket, etc..
        units):
    remove_actions = []
    for unit in units:
        _fail_if_path(unit, "Unmask Unit")

        remove_actions.append(
            image.remove(
                paths.join(ADMIN_ROOT, unit),
            ),
        )

    return remove_actions

# Generate an image feature that enables a unit in the specified systemd target.
def _enable_unit(
        # The name of the systemd unit to enable.  This should be in the
        # full form of the service, ie:  unit.service, unit.mount, unit.socket, etc..
        unit,

        # The systemd target to enable the unit in.
        target = "default.target"):
    _fail_if_path(unit, "Enable Unit")

    return [
        image.ensure_subdirs_exist(PROVIDER_ROOT, target + ".wants", mode = 0o755),
        image.symlink_file(
            paths.join(PROVIDER_ROOT, unit),
            paths.join(PROVIDER_ROOT, target + ".wants", unit),
        ),
    ]

def _install_unit(
        # The source for the unit to be installed. This can be one of:
        #   - A Buck target definition, ie: //some/dir:target or :local-target.
        #   - A filename relative to the current TARGETS file.
        source,

        # The destination service name.  This should be only a single filename,
        # not a path.  The dir the source file is installed into is determinted by
        # the `install_root` parameter.
        dest = None,

        # The dir to install the sysemd unit into.  In most cases this doesn't need
        # to be changed.
        install_root = PROVIDER_ROOT):
    # We haven't been provided an explicit dest so let's try and derive one from the
    # source
    if dest == None:
        if types.is_string(source):
            if ":" in source:
                # `source` appears to be a target, lets see if we can derive the base
                # filename from it and use it as dest.
                dest = target_utils.parse_target(source).name
            else:
                # If it's not a buck target name but it's a string, then we
                # must assume it's a file path that will ulimately be exported
                # as a target via `maybe_export_file`.
                dest = paths.basename(source)

        elif source.path != None:
            # use the `path` part of what should be an `image.source`
            dest = paths.basename(source.path)
        elif source.source != None:
            # use the `source` part of what should be an `image.source`
            dest = target_utils.parse_target(source.source).name
        else:
            fail("Unable to derive `dest` from source: " + source)

    _fail_if_path(dest, "Install Unit Dest")

    return image.install(
        source,
        paths.join(install_root, dest),
    )

def _set_default_target(
        # An existing systemd target to be set as the default
        target):
    return image.symlink_file(
        paths.join(PROVIDER_ROOT, target),
        paths.join(PROVIDER_ROOT, "default.target"),
    )

_ALPHA = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
_NUM = "1234567890"
_SPECIAL = ":_.\\"
_PASSTHROUGH = _ALPHA + _NUM + _SPECIAL

# The Starlark runtime in Buck does not provide `ord()`, so include an explicit map
# Generated with:
# ranges = itertools.chain(range(33, 48), range(58, 65), range(91, 97), range(123,127))
# {chr(i): f"\\x{i:x}" for i in ranges}
_ESCAPE_MAP = {
    "!": "\\x21",
    '"': "\\x22",
    "#": "\\x23",
    "$": "\\x24",
    "%": "\\x25",
    "&": "\\x26",
    "'": "\\x27",
    "(": "\\x28",
    ")": "\\x29",
    "*": "\\x2a",
    "+": "\\x2b",
    ",": "\\x2c",
    "-": "\\x2d",
    ".": "\\x2e",
    "/": "\\x2f",
    ":": "\\x3a",
    ";": "\\x3b",
    "<": "\\x3c",
    "=": "\\x3d",
    ">": "\\x3e",
    "?": "\\x3f",
    "@": "\\x40",
    "[": "\\x5b",
    "\\": "\\x5c",
    "]": "\\x5d",
    "^": "\\x5e",
    "`": "\\x60",
    "{": "\\x7b",
    "|": "\\x7c",
    "}": "\\x7d",
    "~": "\\x7e",
    "_": "\\x5f",
}

def _escape(unescaped, path = False):
    escaped = ""
    if path and unescaped == "/":
        return "-"
    if path:
        unescaped = unescaped.lstrip("/")
        unescaped = unescaped.replace("//", "/")

    # strings in starlark are not iterable, but have an .elems() function to
    # get a character iterator
    if hasattr(unescaped, "elems"):
        unescaped = unescaped.elems()
    for char in unescaped:
        if char in _PASSTHROUGH:
            escaped += char
        elif char == "/":
            escaped += "-"
        elif char in _ESCAPE_MAP:
            escaped += _ESCAPE_MAP[char]
        else:
            fail("'{}' cannot be escaped".format(char))
    return escaped

# Define shapes for systemd units. This is not intended to be an exhaustive
# list of every systemd unit setting from the start, but should be added to as
# more use cases generate units with these shapes.
unit_t = shape.shape(
    description = str,
    requires = shape.list(str, default = []),
    after = shape.list(str, default = []),
    before = shape.list(str, default = []),
)

mount_t = shape.shape(
    unit = unit_t,
    what = str,
    where = shape.path(),
    # add more filesystem types here as required
    type = shape.enum("btrfs", "9p", optional = True),
    options = shape.list(str, default = []),
)

def _mount_unit_file(name, mount):
    return shape.render_template(
        name = name,
        shape = mount_t,
        instance = mount,
        template = "//antlir/bzl/linux/systemd:mount",
    )

systemd = struct(
    enable_unit = _enable_unit,
    install_unit = _install_unit,
    mask_tmpfiles = _mask_tmpfiles,
    mask_units = _mask_units,
    set_default_target = _set_default_target,
    unmask_units = _unmask_units,
    escape = _escape,
    units = struct(
        unit = unit_t,
        mount = mount_t,
        mount_file = _mount_unit_file,
    ),
)

# verified with `systemd-escape`
def _selftest():
    inputs = [
        ("/dev/sda", True, "dev-sda"),
        ("/", True, "-"),
        ("/some//path", True, "some-path"),
        ("https://[face::booc]/path-dash", False, "https:--\\x5bface::booc\\x5d-path\\x2ddash"),
    ]
    for unescaped, path, expected in inputs:
        actual = systemd.escape(unescaped, path)
        if actual != expected:
            fail("expected systemd.escape('{}', path={}) to return '{}' not '{}".format(unescaped, path, expected, actual))

_selftest()
