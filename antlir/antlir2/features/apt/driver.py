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

Must be run inside a Debian chroot that has dpkg-deb installed.
"""

import json
import os
import subprocess
import sys


def main() -> int:
    spec = json.load(sys.stdin)

    mode = spec["mode"]
    assert mode == "run", f"driver.py only supports mode=run, got {mode}"

    install_root = spec["install_root"]
    archive_dir = spec["archive_dir"]
    debs_dir = spec.get("debs_dir", archive_dir)
    resolved_transaction = spec["resolved_transaction"]

    install_pkgs = resolved_transaction.get("install", [])
    remove_pkgs = resolved_transaction.get("remove", [])

    # Ensure dpkg status file and directories exist
    dpkg_dir = os.path.join(install_root, "var", "lib", "dpkg")
    os.makedirs(dpkg_dir, exist_ok=True)
    status_path = os.path.join(dpkg_dir, "status")
    if not os.path.exists(status_path):
        with open(status_path, "w") as f:
            pass
    info_dir = os.path.join(dpkg_dir, "info")
    os.makedirs(info_dir, exist_ok=True)
    updates_dir = os.path.join(dpkg_dir, "updates")
    os.makedirs(updates_dir, exist_ok=True)
    available_path = os.path.join(dpkg_dir, "available")
    if not os.path.exists(available_path):
        with open(available_path, "w") as f:
            pass

    # Handle removals by directly removing files and updating the status db.
    # We don't use `dpkg --remove` because it tries to execute maintainer
    # scripts which fail in a cross-root context.
    for pkg in remove_pkgs:
        pkg_name = pkg["name"]
        json.dump(
            {"tx_item": {"package": pkg, "operation": "remove"}},
            sys.stdout,
        )
        sys.stdout.write("\n")
        sys.stdout.flush()

        # Read the file list and remove files (in reverse order: files before dirs)
        list_path = os.path.join(info_dir, f"{pkg_name}.list")
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

        # Remove dpkg info files for this package
        for f in os.listdir(info_dir):
            if f.startswith(pkg_name + ".") or f == pkg_name:
                try:
                    os.unlink(os.path.join(info_dir, f))
                except OSError:
                    pass

        # Update the status database: mark as not installed
        if os.path.exists(status_path):
            with open(status_path, "r") as f:
                status_content = f.read()
            # Remove the package's entry from the status file
            new_entries = []
            current_entry = []
            for line in status_content.split("\n"):
                if line == "" and current_entry:
                    entry_text = "\n".join(current_entry)
                    # Check if this entry is for the package being removed
                    is_target = False
                    for el in current_entry:
                        if (
                            el.startswith("Package: ")
                            and el.split(": ", 1)[1] == pkg_name
                        ):
                            is_target = True
                            break
                    if not is_target:
                        new_entries.append(entry_text)
                    current_entry = []
                else:
                    current_entry.append(line)
            if current_entry:
                entry_text = "\n".join(current_entry)
                is_target = False
                for el in current_entry:
                    if el.startswith("Package: ") and el.split(": ", 1)[1] == pkg_name:
                        is_target = True
                        break
                if not is_target:
                    new_entries.append(entry_text)
            with open(status_path, "w") as f:
                f.write("\n\n".join(new_entries))
                if new_entries:
                    f.write("\n")

    # Install packages by extracting .deb files directly from the archive dir
    for pkg_info in install_pkgs:
        pkg = pkg_info["package"]
        json.dump(
            {"tx_item": {"package": pkg, "operation": "install"}},
            sys.stdout,
        )
        sys.stdout.write("\n")
        sys.stdout.flush()

        filename = pkg.get("filename", "")
        if not filename:
            json.dump(
                {"tx_error": f"No filename for package {pkg['name']}"},
                sys.stdout,
            )
            sys.stdout.write("\n")
            sys.stdout.flush()
            return 1

        deb_path = os.path.join(debs_dir, filename)
        if not os.path.exists(deb_path):
            json.dump(
                {"tx_error": f"Missing .deb file for {pkg['name']}: {deb_path}"},
                sys.stdout,
            )
            sys.stdout.write("\n")
            sys.stdout.flush()
            return 1

        # Extract the package contents directly into the install root
        result = subprocess.run(
            ["dpkg-deb", "--extract", deb_path, install_root],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            json.dump(
                {"tx_error": f"Failed to extract {pkg['name']}: {result.stderr}"},
                sys.stdout,
            )
            sys.stdout.write("\n")
            sys.stdout.flush()
            return 1

        # Extract control files to the dpkg info directory
        pkg_name = pkg["name"]
        # dpkg uses just the package name for info files on the native arch
        list_name = pkg_name
        ctrl_dir = os.path.join(info_dir, pkg_name + ".tmp")
        os.makedirs(ctrl_dir, exist_ok=True)
        result = subprocess.run(
            ["dpkg-deb", "--control", deb_path, ctrl_dir],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            json.dump(
                {
                    "tx_error": f"Failed to extract control for {pkg_name}: {result.stderr}"
                },
                sys.stdout,
            )
            sys.stdout.write("\n")
            sys.stdout.flush()
            return 1

        # Move control files to proper dpkg info locations
        for ctrl_file in os.listdir(ctrl_dir):
            src = os.path.join(ctrl_dir, ctrl_file)
            if ctrl_file == "control":
                # Don't move control file - it goes into the status db
                continue
            dst = os.path.join(info_dir, f"{list_name}.{ctrl_file}")
            os.rename(src, dst)

        # Generate the file list for this package
        list_path = os.path.join(info_dir, f"{list_name}.list")
        result = subprocess.run(
            ["dpkg-deb", "--contents", deb_path],
            capture_output=True,
            text=True,
        )
        if result.returncode == 0:
            with open(list_path, "w") as f:
                for line in result.stdout.strip().split("\n"):
                    if not line:
                        continue
                    # dpkg-deb --contents output format:
                    # permissions owner/group size date time path [-> link]
                    parts = line.split()
                    if len(parts) >= 6:
                        path = parts[5]
                        # Normalize: remove leading ./ and ensure leading /
                        if path == "./":
                            path = "/."
                        elif path.startswith("./"):
                            path = path[1:]
                        if not path.startswith("/"):
                            path = "/" + path
                        # Remove trailing slashes for directories (but not root)
                        if len(path) > 1 and path.endswith("/"):
                            path = path.rstrip("/")
                        f.write(path + "\n")

        # Read the control file and add the package to the status db
        control_path = os.path.join(ctrl_dir, "control")
        if os.path.exists(control_path):
            with open(control_path, "r") as f:
                control_data = f.read().strip()
            with open(status_path, "a") as f:
                f.write(control_data + "\n")
                f.write("Status: install ok installed\n")
                f.write("\n")

        # Clean up temp control dir
        import shutil

        shutil.rmtree(ctrl_dir, ignore_errors=True)

    return 0


if __name__ == "__main__":
    sys.exit(main())
