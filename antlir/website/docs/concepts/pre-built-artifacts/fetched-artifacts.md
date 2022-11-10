---
id: fetched-artifacts
title: Fetched Artifacts
---

While Antlir provides utilities to build and install binaries directly into images as part of image creation, it can also be advantageous to include "fetched" artifacts. These provide two core benefits:

- **Speed**: One can explicitly cache large, infrequently changed artifacts.
- **Controlled release cadence**: An image may want to use a "stable", released version of a given dependency rather than build it directly alongside the image build.

However, fetched packages are naturally opposed to Antlir's underlying goal of hermeticity and determinism, in that any fetch to an external artifact store is not guaranteed to deterministically produce the same result.

To tackle this, Antlir relies on the concept of "in-repo databases", which:

- Store an address and cryptographic hash of each fetched artifact in a file in the repo.
- Provide a build target that fetches the package, checks the hash, and presents the package contents to be used by images.

The above process is hermetic (unless the package is unavailable). Antlir accomplishes this by:

- Exposing an interface that can then be implemented by different artifact stores. The code is well documented and can be found in [fetched_package_layer.bzl](https://www.internalfb.com/intern/diffusion/FBS/browse/master/fbcode/antlir/bzl/fetched_package_layer.bzl).
- Providing a binary that can be called to update the in-repo database for a given artifact: [update_package_db.py](https://www.internalfb.com/intern/diffusion/FBS/browse/master/fbcode/antlir/update_package_db.py).
