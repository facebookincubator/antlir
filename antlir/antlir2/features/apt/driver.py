#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Compile-time APT driver.

This script is invoked by the apt feature plugin at compile time. It reads
a JSON spec from stdin containing the resolved transaction and installs
the packages into the install root by extracting .deb files directly.

The .deb files are expected to be in the archive directory, accessible via
the `filename` field in each package's resolved metadata.

This only extracts files and updates the dpkg database, as `dpkg --unpack`
would: it honours the path filters in the image's dpkg configuration and the
diversions in its dpkg database. Maintainer scripts are run separately by the
apt feature, inside the image root.

`stage` selects what to do with the packages to install:
- `control`: extract each package's control files to
  /var/lib/dpkg/antlir2-control/<package>, which the apt feature moves to
  /var/lib/dpkg/tmp.ci to run preinst from, as dpkg does
- `extract`: extract their files only, recording nothing, as debootstrap does
  before it installs the packages of a new root
- `unpack` (default): unpack them

Must be run inside a Debian chroot that has dpkg-deb installed.
"""

import fnmatch
import glob
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
from collections.abc import Callable


def _emit(event: dict) -> None:
    json.dump(event, sys.stdout)
    sys.stdout.write("\n")
    sys.stdout.flush()


def _remove_status_entry(status_path: str, pkg_name: str) -> None:
    with open(status_path, "r") as f:
        stanzas = [s for s in f.read().split("\n\n") if s.strip()]
    kept = [
        s for s in stanzas if f"Package: {pkg_name}" not in s.strip("\n").split("\n")
    ]
    with open(status_path, "w") as f:
        f.write("\n\n".join(kept))
        if kept:
            f.write("\n\n")


def _path_filters(install_root: str) -> list[tuple[bool, str]]:
    """
    (include, glob) for the path-include and path-exclude options in the
    image's dpkg configuration, in the order dpkg reads them
    """
    etc = os.path.join(install_root, "etc", "dpkg")
    files = [os.path.join(etc, "dpkg.cfg")] + sorted(
        p
        for p in glob.glob(os.path.join(etc, "dpkg.cfg.d", "*"))
        if os.path.basename(p).replace("-", "").replace("_", "").isalnum()
    )
    filters = []
    for path in files:
        if not os.path.isfile(path):
            continue
        with open(path) as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                name, _, value = (
                    line.partition("=") if "=" in line else line.partition(" ")
                )
                name = name.strip().removeprefix("--")
                if name in ("path-include", "path-exclude"):
                    filters.append((name == "path-include", value.strip()))
    return filters


def _filtered_out(
    path: str, is_dir_or_link: bool, filters: list[tuple[bool, str]]
) -> bool:
    """dpkg's filter_should_skip(); `path` is absolute"""
    skip = False
    for include, pattern in filters:
        if fnmatch.fnmatchcase(path, pattern):
            skip = not include
    if skip and is_dir_or_link:
        # directories (and symlinks to them) an include could bring files
        # back under are kept
        for include, pattern in filters:
            if not include:
                continue
            literal = len(pattern)
            for i, c in enumerate(pattern):
                if c in "*?[\\":
                    literal = i
                    break
            if path.startswith(pattern[:literal].rstrip("/")):
                return False
    return skip


def _diversions(install_root: str) -> dict[str, tuple[str, str]]:
    """path -> (diverted-to path, diverting package, ":" for a local one)"""
    path = os.path.join(install_root, "var", "lib", "dpkg", "diversions")
    if not os.path.exists(path):
        return {}
    with open(path) as f:
        lines = f.read().split("\n")
    return {lines[i]: (lines[i + 1], lines[i + 2]) for i in range(0, len(lines) - 2, 3)}


def _installed_path(
    path: str, pkg_name: str, diversions: dict[str, tuple[str, str]]
) -> str:
    """where dpkg puts `path` of `pkg_name`: its diversion, unless the package diverted it"""
    to, by = diversions.get(path, (None, None))
    return to if to is not None and by != pkg_name else path


def _member_path(name: str) -> str:
    """a tar member name as dpkg records it: absolute, no trailing slash"""
    if name in (".", "./"):
        return "/."
    path = "/" + name.removeprefix("./").lstrip("/")
    return path.rstrip("/") if len(path) > 1 else path


def _extract(
    deb_path: str,
    install_root: str,
    pkg_name: str,
    filters: list[tuple[bool, str]],
    diversions: dict[str, tuple[str, str]],
) -> list[str]:
    """
    Extract a deb's files the way `dpkg --unpack` does, and return the paths
    it ships (all of them, as dpkg lists them in <package>.list)
    """
    with tempfile.TemporaryFile() as tar_stream:
        dpkg_result = subprocess.run(
            ["dpkg-deb", "--fsys-tarfile", deb_path],
            stdout=tar_stream,
            stderr=subprocess.PIPE,
        )
        if dpkg_result.returncode != 0:
            err = dpkg_result.stderr.decode(errors="replace").strip()
            raise RuntimeError(f"Failed to extract {pkg_name}: {err}")
        tar_stream.seek(0)
        with tarfile.open(fileobj=tar_stream) as tar:
            members = tar.getmembers()
            paths = [_member_path(m.name) for m in members]
            extract = []
            for member, path in zip(members, paths, strict=True):
                if path != "/." and _filtered_out(
                    path, member.isdir() or member.issym(), filters
                ):
                    continue
                installed = _installed_path(path, pkg_name, diversions)
                if installed != path:
                    member.name = "." + installed
                    path = installed
                if member.islnk():
                    # a hard link to a diverted file links to the diversion
                    link = _member_path(member.linkname)
                    member.linkname = "." + _installed_path(link, pkg_name, diversions)
                target = os.path.join(install_root, path.lstrip("/"))
                # dpkg replaces what is in the way, but keeps a directory (or
                # a symlink to one, like the merged-/usr aliases from
                # base-files) where the package ships a directory
                if member.isdir():
                    if os.path.lexists(target) and not os.path.isdir(target):
                        os.unlink(target)
                elif os.path.islink(target) or (
                    os.path.lexists(target) and not os.path.isdir(target)
                ):
                    os.unlink(target)
                extract.append(member)
            try:
                tar.extractall(
                    install_root,
                    members=extract,
                    numeric_owner=True,
                    filter="fully_trusted",
                )
            except (OSError, tarfile.TarError) as e:
                raise RuntimeError(f"Failed to extract {pkg_name}: {e}") from e
    return paths


def _md5(path: str) -> str:
    if not os.path.isfile(path):
        return "newconffile"
    with open(path, "rb") as f:
        # dpkg's status database records each conffile's md5sum
        # patternlint-disable-next-line poor-choice-of-hash-function
        return hashlib.md5(f.read()).hexdigest()


def _conffile_entry(
    line: str, install_root: str, installed_path: Callable[[str], str]
) -> str:
    """
    A conffile's line in the status database: its md5sum as unpacked (where
    `installed_path` puts it), or for a `remove-on-upgrade` conffile (which
    the package does not ship) dpkg's placeholder and the flag
    """
    if line.startswith("remove-on-upgrade "):
        return f" {line.split(maxsplit=1)[1]} newconffile remove-on-upgrade"
    path = installed_path(line).lstrip("/")
    return f" {line} {_md5(os.path.join(install_root, path))}"


def _status_stanza(
    control: str,
    conffiles: list[str],
    install_root: str,
    installed_path: Callable[[str], str],
) -> str:
    """
    The status entry for an unpacked package: its control fields, Status, and
    the md5sum of each conffile as unpacked (where `installed_path` puts it)
    """
    fields = control.strip().split("\n")
    lines = []
    for line in fields:
        if line.startswith("Description:") and conffiles:
            lines.append("Conffiles:")
            lines.extend(
                _conffile_entry(c, install_root, installed_path) for c in conffiles
            )
        lines.append(line)
    if conffiles and not any(line.startswith("Description:") for line in fields):
        lines.append("Conffiles:")
        lines.extend(
            _conffile_entry(c, install_root, installed_path) for c in conffiles
        )
    lines.append("Status: install ok installed")
    return "\n".join(lines) + "\n\n"


def _file_trigger_interests(dpkg_dir: str, pkg_name: str, triggers: str | None) -> None:
    """
    Record the paths `pkg_name` is interested in (its triggers control file,
    or none) in triggers/File, as dpkg does: its old entries go, and the new
    ones are added after everyone else's, so the file lists packages in the
    order they were unpacked.
    """
    bare = pkg_name.partition(":")[0]
    interests = []
    if triggers is not None and os.path.exists(triggers):
        with open(triggers) as f:
            for line in f:
                words = line.split("#", 1)[0].split()
                if len(words) == 2 and words[1].startswith("/"):
                    if words[0] in ("interest", "interest-await"):
                        interests.append(f"{words[1]} {pkg_name}")
                    elif words[0] == "interest-noawait":
                        interests.append(f"{words[1]} {pkg_name}/noawait")
    path = os.path.join(dpkg_dir, "triggers", "File")
    lines = []
    if os.path.exists(path):
        with open(path) as f:
            lines = f.read().splitlines()
    kept = [
        line
        for line in lines
        if line.partition(" ")[2].partition("/")[0].partition(":")[0] != bare
    ]
    if kept == lines and not interests:
        return
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.writelines(line + "\n" for line in kept + interests)


def _remove(pkg: dict, install_root: str, info_dir: str, status_path: str) -> None:  # noqa: C901
    pkg_name = pkg["name"]
    _emit({"tx_item": {"package": pkg, "operation": "remove"}})

    # Read the file list and remove files (in reverse order: files before dirs)
    list_path = os.path.join(info_dir, f"{pkg_name}.list")
    if not os.path.exists(list_path):
        list_path = os.path.join(info_dir, f"{pkg_name}:{pkg['arch']}.list")
    if os.path.exists(list_path):
        with open(list_path, "r") as f:
            paths = [line.strip() for line in f if line.strip()]
        # Remove files first (reverse so we get files before their parent dirs)
        for path in reversed(paths):
            full_path = os.path.join(install_root, path.lstrip("/"))
            if os.path.isfile(full_path) or os.path.islink(full_path):
                try:
                    os.unlink(full_path)
                except OSError:
                    pass
            elif os.path.isdir(full_path):
                try:
                    os.rmdir(full_path)  # Only removes empty dirs
                except OSError:
                    pass

    _file_trigger_interests(os.path.dirname(info_dir), pkg_name, None)

    # Remove dpkg info files for this package
    for f in os.listdir(info_dir):
        if f.startswith(pkg_name + ".") or f.startswith(pkg_name + ":"):
            try:
                os.unlink(os.path.join(info_dir, f))
            except OSError:
                pass

    _remove_status_entry(status_path, pkg_name)


def _infodb_multiarch(info_dir: str) -> bool:
    """
    Whether the info database is in dpkg's format 1, which names the files of
    Multi-Arch: same packages with their architecture. A new one starts in
    it, as dpkg starts one; dpkg upgrades an older one when it next runs.
    """
    path = os.path.join(info_dir, "format")
    if not os.path.exists(path):
        return False
    with open(path) as f:
        return f.read().strip() == "1"


def _init_infodb_format(info_dir: str) -> None:
    """
    Start a new info database in format 1, before the transaction writes any
    package's files to it (an existing one without `format` is format 0)
    """
    path = os.path.join(info_dir, "format")
    if not os.path.exists(path) and not any(
        f.endswith(".list") for f in os.listdir(info_dir)
    ):
        with open(path, "w") as f:
            f.write("1\n")


def _extract_control(deb_path: str, ctrl_dir: str, pkg_name: str) -> None:
    shutil.rmtree(ctrl_dir, ignore_errors=True)
    os.makedirs(ctrl_dir)
    result = subprocess.run(
        ["dpkg-deb", "--control", deb_path, ctrl_dir],
        capture_output=True,
        text=True,
        errors="replace",
    )
    if result.returncode != 0:
        raise RuntimeError(f"Failed to extract control for {pkg_name}: {result.stderr}")


def _install(
    pkg: dict,
    deb_path: str,
    stage: str,
    install_root: str,
    dpkg_dir: str,
    filters: list[tuple[bool, str]],
) -> None:
    pkg_name = pkg["name"]
    info_dir = os.path.join(dpkg_dir, "info")
    ctrl_dir = os.path.join(dpkg_dir, "antlir2-control", pkg_name)

    if stage == "control":
        _extract_control(deb_path, ctrl_dir, pkg_name)
        return
    if stage == "extract":
        _extract(deb_path, install_root, pkg_name, filters, {})
        return

    _emit({"tx_item": {"package": pkg, "operation": "install"}})
    if not os.path.isdir(ctrl_dir):
        _extract_control(deb_path, ctrl_dir, pkg_name)
    diversions = _diversions(install_root)
    paths = _extract(deb_path, install_root, pkg_name, filters, diversions)

    control_path = os.path.join(ctrl_dir, "control")
    control_data = ""
    if os.path.exists(control_path):
        with open(control_path, "r") as f:
            control_data = f.read()
    # dpkg names the info files of a Multi-Arch: same package (and its
    # entries in other databases) with its architecture
    info_name = pkg_name
    multi_arch_same = "\nMulti-Arch: same\n" in f"\n{control_data}"
    if ":" not in pkg_name and multi_arch_same and _infodb_multiarch(info_dir):
        info_name = f"{pkg_name}:{pkg['arch']}"

    # Move control files to proper dpkg info locations
    for ctrl_file in os.listdir(ctrl_dir):
        if ctrl_file == "control":
            # Don't move control file - it goes into the status db
            continue
        os.rename(
            os.path.join(ctrl_dir, ctrl_file),
            os.path.join(info_dir, f"{info_name}.{ctrl_file}"),
        )

    with open(os.path.join(info_dir, f"{info_name}.list"), "w") as f:
        f.writelines(path + "\n" for path in paths)
    _file_trigger_interests(
        dpkg_dir, info_name, os.path.join(info_dir, f"{info_name}.triggers")
    )

    conffiles_path = os.path.join(info_dir, f"{info_name}.conffiles")
    conffiles = []
    if os.path.exists(conffiles_path):
        with open(conffiles_path) as f:
            conffiles = [
                line.strip()
                for line in f
                if line.strip().startswith(("/", "remove-on-upgrade "))
            ]

    if control_data:
        # Reinstalls and upgrades must replace the old entry: dpkg refuses
        # to parse a status file with two instances of one package.
        status_path = os.path.join(dpkg_dir, "status")
        _remove_status_entry(status_path, pkg_name)
        with open(status_path, "a") as f:
            f.write(
                _status_stanza(
                    control_data,
                    conffiles,
                    install_root,
                    lambda c: _installed_path(c, pkg_name, diversions),
                )
            )

    shutil.rmtree(ctrl_dir, ignore_errors=True)


def main() -> int:
    spec = json.load(sys.stdin)

    mode = spec["mode"]
    assert mode == "run", f"driver.py only supports mode=run, got {mode}"
    stage = spec.get("stage") or "unpack"

    install_root = spec["install_root"]
    archive_dir = spec["archive_dir"]
    debs_dir = spec.get("debs_dir") or archive_dir
    resolved_transaction = spec["resolved_transaction"]

    install_pkgs = resolved_transaction.get("install", [])
    remove_pkgs = resolved_transaction.get("remove", [])

    # Ensure dpkg status file and directories exist
    dpkg_dir = os.path.join(install_root, "var", "lib", "dpkg")
    os.makedirs(dpkg_dir, exist_ok=True)
    status_path = os.path.join(dpkg_dir, "status")
    if not os.path.exists(status_path):
        open(status_path, "w").close()
    info_dir = os.path.join(dpkg_dir, "info")
    os.makedirs(info_dir, exist_ok=True)
    _init_infodb_format(info_dir)
    updates_dir = os.path.join(dpkg_dir, "updates")
    os.makedirs(updates_dir, exist_ok=True)
    available_path = os.path.join(dpkg_dir, "available")
    if not os.path.exists(available_path):
        open(available_path, "w").close()

    # Maintainer scripts (prerm/postrm) are run by the apt feature around this
    # step, inside the image root rather than this build appliance, so this
    # must not use `dpkg --remove`, which would also run them.
    for pkg in remove_pkgs:
        _remove(pkg, install_root, info_dir, status_path)

    filters = _path_filters(install_root)
    for pkg_info in install_pkgs:
        pkg = pkg_info["package"]
        filename = pkg.get("filename", "")
        if not filename:
            _emit({"tx_error": f"No filename for package {pkg['name']}"})
            return 1
        deb_path = os.path.join(debs_dir, filename)
        if not os.path.exists(deb_path):
            _emit({"tx_error": f"Missing .deb file for {pkg['name']}: {deb_path}"})
            return 1
        try:
            _install(pkg, deb_path, stage, install_root, dpkg_dir, filters)
        except RuntimeError as e:
            _emit({"tx_error": str(e)})
            return 1

    control_dir = os.path.join(dpkg_dir, "antlir2-control")
    if (
        stage != "control"
        and os.path.isdir(control_dir)
        and not os.listdir(control_dir)
    ):
        os.rmdir(control_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
