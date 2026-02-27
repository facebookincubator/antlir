#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Resolve apt transactions using python-apt against a local snapshot archive.

This script is invoked by the plan binary. It reads a JSON spec from stdin
and outputs the resolved transaction as line-delimited JSON events to stdout.

Must be run inside a Debian chroot that has python3-apt installed.
"""

import json
import os
import shutil
import sys
import tempfile

import apt_pkg


def main() -> int:
    spec = json.load(sys.stdin)

    archive_dir = spec["archive_dir"]
    install_root = spec["install_root"]
    items = spec["items"]
    arch = spec.get("arch")
    mode = spec["mode"]
    dpkg_status_content = spec.get("dpkg_status")

    assert mode == "resolve", f"resolve.py only supports mode=resolve, got {mode}"

    if not os.path.isdir(archive_dir):
        print(f"Error: archive dir {archive_dir} does not exist", file=sys.stderr)
        return 1

    # Discover the distribution name from the archive layout
    dists_dir = os.path.join(archive_dir, "dists")
    if not os.path.isdir(dists_dir):
        print(f"Error: {dists_dir} does not exist", file=sys.stderr)
        return 1
    distributions = os.listdir(dists_dir)
    if len(distributions) != 1:
        print(
            f"Error: expected exactly 1 distribution in {dists_dir}, "
            f"found {distributions}",
            file=sys.stderr,
        )
        return 1
    distribution = distributions[0]

    with tempfile.TemporaryDirectory() as apt_root:
        # Set up the directory structure apt expects
        apt_conf_dir = os.path.join(apt_root, "etc", "apt")
        apt_state_dir = os.path.join(apt_root, "var", "lib", "apt")
        apt_cache_dir = os.path.join(apt_root, "var", "cache", "apt")
        apt_log_dir = os.path.join(apt_root, "var", "log", "apt")
        dpkg_dir = os.path.join(apt_root, "var", "lib", "dpkg")
        trusted_gpg_dir = os.path.join(apt_conf_dir, "trusted.gpg.d")

        for d in [
            apt_conf_dir,
            os.path.join(apt_state_dir, "lists", "partial"),
            os.path.join(apt_cache_dir, "archives", "partial"),
            apt_log_dir,
            dpkg_dir,
            trusted_gpg_dir,
        ]:
            os.makedirs(d, exist_ok=True)

        # Use the parent layer's dpkg status if available, otherwise start empty
        status_path = os.path.join(dpkg_dir, "status")
        if dpkg_status_content is not None:
            with open(status_path, "w") as f:
                f.write(dpkg_status_content)
        else:
            parent_status = os.path.join(install_root, "var", "lib", "dpkg", "status")
            if os.path.exists(parent_status):
                shutil.copy2(parent_status, status_path)
            else:
                with open(status_path, "w") as f:
                    pass

        # Write sources.list pointing at the local archive
        sources_list = os.path.join(apt_conf_dir, "sources.list")
        # Discover components from the dists/<distribution>/ directory
        dist_path = os.path.join(dists_dir, distribution)
        components = [
            d
            for d in os.listdir(dist_path)
            if os.path.isdir(os.path.join(dist_path, d)) and d != "InRelease"
        ]
        components_str = " ".join(sorted(components))
        with open(sources_list, "w") as f:
            f.write(
                f"deb [trusted=yes] file://{archive_dir} {distribution} {components_str}\n"
            )

        # Initialize apt_pkg with our custom root
        apt_pkg.init()
        apt_pkg.config.set("Dir", apt_root)
        apt_pkg.config.set("Dir::Etc", apt_conf_dir)
        apt_pkg.config.set("Dir::Etc::sourcelist", sources_list)
        apt_pkg.config.set("Dir::Etc::sourceparts", "/dev/null")
        apt_pkg.config.set("Dir::State", apt_state_dir)
        apt_pkg.config.set("Dir::Cache", apt_cache_dir)
        apt_pkg.config.set("Dir::Log", apt_log_dir)
        apt_pkg.config.set("Dir::State::status", status_path)
        apt_pkg.config.set("Acquire::AllowInsecurearchivesitories", "true")

        if arch:
            apt_pkg.config.set("APT::Architecture", arch)
            apt_pkg.config.set("APT::Architectures", arch)

        # Update the package lists
        import apt

        cache = apt.Cache(rootdir=apt_root)
        try:
            cache.update()
        except apt.cache.FetchFailedException as e:
            print(f"Warning: apt update had issues: {e}", file=sys.stderr)
        cache.open()

        # Process items
        install_packages = []
        remove_packages = []

        for item in items:
            action = item["action"]
            subject = item["apt"]["subject"]

            if action == "install":
                if subject not in cache:
                    json.dump({"package_not_found": subject}, sys.stdout)
                    sys.stdout.write("\n")
                    continue
                cache[subject].mark_install()
                install_packages.append(subject)
            elif action == "remove":
                if subject not in cache or not cache[subject].is_installed:
                    json.dump({"package_not_installed": subject}, sys.stdout)
                    sys.stdout.write("\n")
                    continue
                cache[subject].mark_delete()
                remove_packages.append(subject)
            elif action == "remove_if_exists":
                if subject in cache and cache[subject].is_installed:
                    cache[subject].mark_delete()
                    remove_packages.append(subject)

        # Get the list of changes
        changes = cache.get_changes()

        install_list = []
        remove_list = []

        for pkg in sorted(changes, key=lambda p: p.name):
            if pkg.marked_install or pkg.marked_upgrade:
                candidate = pkg.candidate
                if candidate is None:
                    continue
                record = candidate.record
                filename = record.get("Filename", "") if record else ""
                sha256 = record.get("SHA256", "") if record else ""
                install_list.append(
                    {
                        "package": {
                            "name": pkg.name,
                            "version": candidate.version,
                            "arch": candidate.architecture,
                            "filename": filename,
                            "sha256": sha256,
                        },
                        "archive": distribution,
                    }
                )
            elif pkg.marked_delete:
                installed = pkg.installed
                pkg_arch = ""
                try:
                    pkg_arch = pkg.architecture
                    # In some versions, architecture is a method
                    if callable(pkg_arch):
                        pkg_arch = pkg_arch()
                except Exception:
                    pass
                remove_list.append(
                    {
                        "name": pkg.name,
                        "version": installed.version if installed else "",
                        "arch": str(pkg_arch),
                    }
                )

        # Compute the resulting dpkg status after the transaction.
        # This is used by child layers via supplements so they don't need
        # the full parent layer subvolume to plan their own apt transactions.
        status_entries = []
        for pkg in sorted(cache, key=lambda p: p.name):
            if pkg.marked_delete:
                continue
            if pkg.marked_install or pkg.marked_upgrade:
                candidate = pkg.candidate
                if candidate:
                    status_entries.append(
                        "Package: {}\n"
                        "Status: install ok installed\n"
                        "Version: {}\n"
                        "Architecture: {}\n".format(
                            pkg.name,
                            candidate.version,
                            candidate.architecture,
                        )
                    )
            elif pkg.is_installed:
                installed = pkg.installed
                if installed:
                    pkg_arch = ""
                    try:
                        pkg_arch = pkg.architecture
                        if callable(pkg_arch):
                            pkg_arch = pkg_arch()
                    except Exception:
                        pass
                    status_entries.append(
                        "Package: {}\n"
                        "Status: install ok installed\n"
                        "Version: {}\n"
                        "Architecture: {}\n".format(
                            pkg.name,
                            installed.version,
                            str(pkg_arch),
                        )
                    )
        dpkg_status_out = "\n".join(status_entries)

        # Output the resolved transaction as a single JSON event
        json.dump(
            {
                "transaction_resolved": {
                    "install": install_list,
                    "remove": remove_list,
                    "dpkg_status": dpkg_status_out,
                }
            },
            sys.stdout,
        )
        sys.stdout.write("\n")
        sys.stdout.flush()

    return 0


if __name__ == "__main__":
    sys.exit(main())
