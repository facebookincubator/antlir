#!/usr/bin/env python3
import argparse
import gzip
import hashlib
import os
import re
import tarfile
from io import BytesIO

import requests
import toml


CRATE_VERSION_RE = re.compile(r"^vendor/(?P<crate>.*)-(?P<ver>\d+.*?)/")

parser = argparse.ArgumentParser()
parser.add_argument("cargo_lock")
parser.add_argument("crate_root")
parser.add_argument("out")


def main():
    args = parser.parse_args()

    with open(args.cargo_lock) as f:
        cargo_lock = toml.load(f)

    match = CRATE_VERSION_RE.match(args.crate_root)
    assert match is not None
    crate = match.group("crate")
    version = match.group("ver")

    os.makedirs(os.path.join(args.out, "vendor"))

    for package in cargo_lock["package"]:
        if package["name"] == crate and package["version"] == version:
            src = package["source"]
            if src == "registry+https://github.com/rust-lang/crates.io-index":
                url = f"https://static.crates.io/crates/{crate}/{crate}-{version}.crate"
            else:
                raise RuntimeError(
                    f"only crates.io deps are supported, got {package}"
                )
            print(url)
            resp = requests.get(url)
            assert resp.status_code == 200, resp.status_code
            assert (
                hashlib.sha256(resp.content).hexdigest() == package["checksum"]
            )
            tar = tarfile.TarFile(
                fileobj=gzip.GzipFile(fileobj=BytesIO(resp.content))
            )
            tar.extractall(os.path.join(args.out, "vendor"))

            return

    raise Exception(f"failed to find {crate}-{version}")


if __name__ == "__main__":
    main()
