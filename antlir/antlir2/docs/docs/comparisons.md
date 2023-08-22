---
sidebar_position: 100
sidebar_label: Comparison with other build systems
---

# antlir2 as compared to other image build systems

## General goals

`antlir2` is tightly integrated with `buck2` so that monorepo artifacts can
easily be built into images combined with upstream packages.

`antlir2` makes it (almost) impossible for non-determinism to leak into image
builds (aside from badly behaved buck rules providing inputs to be installed).

## vs `docker build`

`docker build` with a `Dockerfile` is something that many engineers are familiar
with - it provides a relatively easy to read description about how to build an
image.

### Determinism

`Dockerfile`s almost always have a line like `dnf update -y` before installing
any packages. This means that every time you run `docker build`, you're getting
a live view of the upstream package repos.
While this is generally acceptable by many users, it makes it impossible to
retroactively rebuild an image as it would have been produced by an earlier
build. Forget about bisecting when a dependency changes!

`antlir2` has an SCM-tracked snapshot of upstream package repositories, so that
builds are always perfectly [reproducible](reproducibility.md) on a given SCM
rev.

### Caching

The unit of caching for a `Dockerfile` is each line. If a line changes (or any
of the lines preceding it change) it will be re-run on the next `docker build`.

`antlir2`'s logical layering unit is an `image.layer` target. However, caching
is more intelligent within this level, making it more reasonable to mix package
installation with your own code that depends on those packages, since that
package installation will still be cached.


### Building other artifacts

It's not very interesting to build an image without any of your own software in
it (if that's all you want, a prebuilt `docker` image is probably good enough).

To build your own code in a `Dockerfile` requires you to install any
build dependencies, then copy your source code, run a compiler, move the output
somewhere and then finally (if you don't forget) delete the build dependencies
and source code, leaving a clean image that you actually want to deploy.

In `antlir2`, you can use `buck2` to build whatever you want, and install only
the artifacts that you want in the final image.
