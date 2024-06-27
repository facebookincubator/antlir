# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:paths.bzl", "paths")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load(":build_defs.bzl", "target_utils")
load(":shape.bzl", "shape")
load(":systemd.shape.bzl", "mount_t", "unit_t")
load(":types.bzl", "types")

USER_PROVIDER_ROOT = "/usr/lib/systemd/user"
PROVIDER_ROOT = "/usr/lib/systemd/system"
ADMIN_ROOT = "/etc/systemd/system"
TMPFILES_ROOT = "/etc/tmpfiles.d"

# This is obviously not an exhaustive list, but are the only unit types that we
# care about at the moment
_ALLOWED_UNIT_SUFFIXES = (
    ".device",
    ".mount",
    ".path",
    ".service",
    ".socket",
    ".target",
    ".timer",
    ".conf",
    ".swap",
    ".slice",
)

def _fail_if_path(thing, monkeymsg):
    """ If thing is a path do a big ole fail and prepend the monkey message for
        helping the human with some context in the fail message.
    """
    if "/" in thing:
        fail(monkeymsg + "({}) is a path, that is not allowed".format(thing))
    else:
        return thing

def _assert_unit_suffix(unit):
    _, extension = paths.split_extension(unit)
    if extension not in _ALLOWED_UNIT_SUFFIXES:
        fail("{} is not a valid unit name (unsupported suffix)".format(unit))

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
            feature.ensure_file_symlink(
                link = paths.join(root, item),
                target = "/dev/null",
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
            feature.remove(
                path = paths.join(ADMIN_ROOT, unit),
            ),
        )

    return remove_actions

# Generate an image feature that enables a unit in the specified systemd target.
def _enable_impl(
        # The name of the systemd unit to enable.  This should be in the
        # full form of the service, ie:  unit.service, unit.mount, unit.socket, etc..
        unit,
        # The systemd target to enable the unit in.
        target,
        # Dependency type to create.
        dep_type,
        # The dir the systemd unit was installed in.  In most cases this doesn't need
        # to be changed.
        installed_root,
        # Informational string that describes what is being enabled. Prepended
        # to an error message on path verification failure.
        description):
    _fail_if_path(unit, description)
    _assert_unit_suffix(unit)
    if dep_type not in ("wants", "requires"):
        fail("dep_type must be one of {wants, requires}")

    num_template_seps = unit.count("@")
    if num_template_seps == 0:
        link_target = unit
    elif num_template_seps == 1:
        # From systemd.unit(5) man page:
        # systemctl enable getty@tty2.service creates a
        # getty.target.wants/getty@tty2.service link to getty@.service.
        name_prefix, suffix = paths.split_extension(unit)
        unit_name, sep, _instance_name = name_prefix.rpartition("@")
        link_target = unit_name + sep + suffix
    else:
        fail("unit contains too many @ characters: " + unit)

    return [
        feature.ensure_subdirs_exist(
            into_dir = installed_root,
            subdirs_to_create = target + "." + dep_type,
            mode = 0o755,
        ),
        feature.ensure_file_symlink(
            link = paths.join(installed_root, target + "." + dep_type, unit),
            target = paths.join(installed_root, link_target),
        ),
    ]

# Image feature to enable a system unit
def _enable_unit(
        unit,
        target = "default.target",
        dep_type = "wants",
        installed_root = PROVIDER_ROOT):
    return _enable_impl(unit, target, dep_type, installed_root, "Enable System Unit")

# Image feature to enable a user unit
def _enable_user_unit(
        unit,
        target = "default.target",
        dep_type = "wants",
        installed_root = USER_PROVIDER_ROOT):
    return _enable_impl(unit, target, dep_type, installed_root, "Enable User Unit")

def _install_impl(
        # The source for the unit to be installed. This can be one of:
        #   - A Buck target definition, ie: //some/dir:target or :local-target.
        #   - A filename relative to the current TARGETS file.
        source,

        # The destination service name.  This should be only a single filename,
        # not a path.  The dir the source file is installed into is determinted by
        # the `install_root` parameter.
        dest,

        # The dir to install the systemd unit into.  In most cases this doesn't need
        # to be changed.
        install_root,

        # Informational string that describes what is being installed. Prepended
        # to an error message on path verification failure.
        description,
        # Remove an existing file that conflicts, if one exists
        force = False):
    # We haven't been provided an explicit dest so let's try and derive one from the
    # source
    if dest == None:
        if not types.is_string(source):
            fail("Unable to derive `dest` from source: " + source)
        if ":" in source:
            # `source` appears to be a target, lets see if we can derive the base
            # filename from it and use it as dest.
            dest = target_utils.parse_target(source).name
        else:
            # If it's not a buck target name but it's a string, then we
            # must assume it's a file path that will ulimately be exported
            # as a target via `maybe_export_file`.
            dest = paths.basename(source)

    _fail_if_path(dest, description + " Dest")
    _assert_unit_suffix(dest)

    return [
        feature.install(
            src = source,
            dst = paths.join(install_root, dest),
        ),
    ] + ([feature.remove(
        path = paths.join(install_root, dest),
        must_exist = False,
    )] if force else [])

# Image feature to install a system unit
def _install_unit(
        source,
        dest = None,
        install_root = PROVIDER_ROOT,
        force = False):
    return _install_impl(source, dest, install_root, "Install System Unit", force = force)

# Image feature to install a user unit
def _install_user_unit(
        source,
        dest = None,
        install_root = USER_PROVIDER_ROOT):
    return _install_impl(source, dest, install_root, "Install User Unit")

def _install_dropin(
        # The source for the unit to be installed. This can be one of:
        #   - A Buck target definition, ie: //some/dir:target or :local-target.
        #   - A filename relative to the current TARGETS file.
        source,
        # The unit that this dropin should affect.
        unit,
        # The destination config name. This should only be a single filename, not a full path.
        dest = None,
        # The dir to install the dropin into. In most cases this doesn't need
        # to be changed.
        install_root = PROVIDER_ROOT,
        # Remove an existing file that conflicts, if one exists
        force = False):
    _assert_unit_suffix(unit)

    # We haven't been provided an explicit dest so let's try and derive one from the
    # source
    if dest == None:
        if not types.is_string(source):
            fail("Unable to derive `dest` from source: " + source)
        if ":" in source:
            # `source` appears to be a target, lets see if we can derive the base
            # filename from it and use it as dest.
            dest = target_utils.parse_target(source).name
        else:
            # If it's not a buck target name but it's a string, then we
            # must assume it's a file path that will ulimately be exported
            # as a target via `maybe_export_file`.
            dest = paths.basename(source)

        # for the auto determined dest name, append the right suffix
        if not dest.endswith(".conf"):
            dest += ".conf"
    else:
        # if given explicitly, a user must give the right name
        if not dest.endswith(".conf"):
            fail("dropin files must have the suffix '.conf'")

    _fail_if_path(dest, "Install Dropin Dest")

    dst_path = paths.join(install_root, unit + ".d", dest)
    features = [
        feature.ensure_subdirs_exist(
            into_dir = install_root,
            subdirs_to_create = unit + ".d",
        ),
        feature.install(
            src = source,
            dst = dst_path,
        ),
    ]
    if force:
        features.append(feature.remove(
            path = dst_path,
            must_exist = False,
        ))
    return features

def _remove_dropin(
        # The unit that this dropin should affect.
        unit,
        # The config name. This should only be a single filename, not a full path.
        dest,
        # The dir to install the dropin into. In most cases this doesn't need
        # to be changed.
        install_root = PROVIDER_ROOT):
    _assert_unit_suffix(unit)

    # a user must give the right name
    if not dest.endswith(".conf"):
        fail("dropin files must have the suffix '.conf'")

    _fail_if_path(dest, "Remove Dropin Dest")

    dst_path = paths.join(install_root, unit + ".d", dest)
    return [
        feature.remove(
            path = dst_path,
            must_exist = False,
        ),
    ]

def _set_default_target(
        # An existing systemd target to be set as the default
        target,
        # Delete any default target that may already exist
        force = False):
    features = [
        feature.ensure_file_symlink(
            link = paths.join(PROVIDER_ROOT, "default.target"),
            target = paths.join(PROVIDER_ROOT, target),
        ),
    ]
    if force:
        features.append(feature.remove(
            path = paths.join(PROVIDER_ROOT, "default.target"),
            must_exist = False,
        ))
    return features

def _alias(unit, alias):
    return feature.ensure_file_symlink(
        link = paths.join(ADMIN_ROOT, alias),
        target = paths.join(PROVIDER_ROOT, unit),
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

def _mount_unit_file(name, mount):
    return shape.render_template(
        name = name,
        instance = mount,
        template = "antlir//antlir/bzl/linux/systemd:mount",
    )

def _skip_unit(unit, force = False):
    return _install_dropin("antlir//antlir/bzl:99-skip-unit.conf", unit, force = force)

def _unskip_unit(unit):
    return _remove_dropin(unit, "99-skip-unit.conf")

systemd = struct(
    alias = _alias,
    enable_unit = _enable_unit,
    enable_user_unit = _enable_user_unit,
    escape = _escape,
    install_dropin = _install_dropin,
    install_unit = _install_unit,
    install_user_unit = _install_user_unit,
    mask_tmpfiles = _mask_tmpfiles,
    mask_units = _mask_units,
    remove_dropin = _remove_dropin,
    set_default_target = _set_default_target,
    skip_unit = _skip_unit,
    units = struct(
        mount = mount_t,
        mount_file = _mount_unit_file,
        unit = unit_t,
    ),
    unmask_units = _unmask_units,
    unskip_unit = _unskip_unit,
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
