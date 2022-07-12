---
id: overview
title: Overview
slug: /
---
## What is Antlir?

Antlir can **reproducibly build**, **test**, and **run** OS images for
containers and hosts.

Images are specified as [Buck](https://buck.build/) build targets, which
**declaratively compose** Antlir’s
[Starlark](https://github.com/bazelbuild/starlark) macros.  Our image
language supports inheritance (aka layering), as well as functional
composition.  The API is safe by design:

  - detects filesystem conflicts, and fails at build-time
  - avoids constructs with non-deterministic or implicit behavior
  - prevents sequencing bugs by automatically dependency-sorting actions

Builds are **fast** thanks to a combination of Buck parallelism, caching,
and Antlir’s usage of the `btrfs` copy-on-write filesystem (NB: we use a
loopback, so no need to reformat your host FS).

Antlir supports a variety of **image packaging** styles, including tarballs,
[cpio](https://en.wikipedia.org/wiki/Cpio),
[SquashFS](https://en.wikipedia.org/wiki/SquashFS),
[btrfs](https://btrfs.wiki.kernel.org/index.php/Main_Page) loopbacks and
sendstreams.  We are now working on a package-centric deployment format,
which implicitly shares content between related images, or versions of
images — reducing container update time, and disk usage, and enabling
package-level hotfixes.

## Easy, maintainable, efficient image deployment

Before Antlir, a typical image deployment workflow might look like this:

  - Write a script to compose and package an image.
  - For each new application, copy or refactor the script to accommodate it,
    and build a new redistributable.
  - Struggle with the maintenance weight of undeclared dependencies, code
    duplication, and expensive updates.

Antlir’s feature-set helps you move away from monolithic “kitchen sink”
images, and instead focus on developing, testing, and composing “image
features”.  Features can blend packaging and configuration management,
specifying either the binaries to deploy, or their configuration, or both.
Antlir does not prevent the use of runtime configuration management, but it
makes it easy to do what *can* be done at build-time, so that your
application can be tested and deployed safely.  With Antlir, you would:

  - Split the commonly used parts of your OS filesystem into layers, for
    better build speed and uniformity of infrastructure.  Layers are a good
    point of ownership, since they can be tested and released as pre-built
    artifacts.
  - Compose the layers from features, maximizing code clarity and reuse.
    Each feature can be tested with minimal images — this is the
    image-building analog of unit tests.
  - Specialize the few shared layers into many per-application layers.  Each
    application gains clear dependencies, and control of its release cycle.
    Since the parent layers and features have their own tests, the
    application owner can focus on testing their integration, without
    worrying about the OS.  Lastly, with the upcoming package-centric
    deployment, each application will only pay for the new bytes that it
    adds to the image.

## Reproducibility

For a fixed source control revision, `buck build //image:target` should
produce a **functionally identical** image, no matter who runs the build, or
when.  For now, images are *not* bitwise-reproducible:

  - We make no effort to eliminate C/C++ compiler entropy,
  - We do not prevent the embedding of “build info”[^build_info] (such as
    time, host, source control, etc) into the build output.

However, we do go to significant lengths to eliminate functional variation.
Specifically:

  - Image assembly happens in containers with no network access.  Use
    [`http_file`](https://buck.build/rule/http_file.html) for out-of-repo
    resources.
  - RPM installs do not talk to “live” repos, which can change at any time.
    Instead, we commit a reference to a fixed “repo snapshot” into source
    control, and use that to install repo-deterministic packages.  As part
    of the default “build appliance” (**TODO** link to definition), we
    maintain snapshots for commonly-used distros.  If you need your own repo
    snapshots, Antlir comes with the tools to maintain them.
  - Buck is designed for repo-deterministic builds — its distributed cache
    semantics will break if a build is not repo-hermetic.  Specifically, if
    host *A* builds and caches an artifact, and host *B* later uses it, the
    artifact must be functionally substitutable, or host *B*’s output will
    be incorrect.  So, the output of each Buck rule must only depend on its
    inputs, and the inputs themselves should be functions of the current
    repo rev, Buck config options, and nothing else.

### Benefits of reproducibly built images

  - **Easy-to-debug production builds.** If your image is misbehaving, just
    check out the source control revision that built it, and make a patch to
    instrument the build.  Now you have an instrumented image that is very
    likely to reproduce your problem.
  - **Safer hotfixes.** With images, services get predictable filesystems:
    *what you build is what you test, is what you deploy*.  Let’s say you
    find a bug in production, and need to apply a small patch to fix it.  If
    your build is not reproducible, rebuilding with the hotfix can bring in
    unrelated changes, turning your hotfix into a larger outage.
  - **Bisects from source control.** For any service breakage, whether
    caused by your binary, or by its OS dependencies, you can use the
    “bisect” feature of your version-control system to easily find the
    offending change.
  - **Auditing of build artifacts.** Given trusted source code, and a built
    artifact, reproducibility is essential for validating that the artifact
    was in fact built from the source.  This lets you monitor for
    compromised build hosts.

## Pre-built artifacts

A common development pattern is that you have one team maintaining a layer,
which is used by many other teams.

In such a case, it is possible that a single change to this parent layer
will break many child layers.  It can therefore be preferable for the common
image to be built and tested from source control, and then released
gradually to customers as a pre-build artifact.

Releasing common image layers as pre-built artifacts can also speed up build
time for the teams that depend on it.

For a concrete example of a pre-built artifact, consider the “build
appliance” image (**TODO** link).  This is a pre-built image, which contains
all that is necessary to build new images, including tooling and RPM repo
snapshots.

Antlir comes with first-class support for pre-built images, including:

  - fetching them from external blob-stores & validating checksums (**TODO**
    link to impl)
  - using them in builds — just import your downloaded blob to present an
    `image.layer`-like build-time interface, see e.g.
    `image.layer_from_package`
  - packaging them via `image.package`
  - publishing them to a blob store via `buck run` (**TODO** link to impl)

In effect, Antlir comes with all the tools to maintain a basic image
registry.  This of course does not prevent integration with third-party
image registries, but simply exists to allow for reproducible,
closed-ecosystem builds that involve pre-built artifacts. Read more in the
[pre-built artifacts section](
concepts/pre-built-artifacts/fetched-artifacts).

## Bonus features of Antlir

While these applications are slightly outside of the “build OS images”
mission, they are core to the implementation, and will likely be supported
indefinitely.

  - **Build sandbox:** The Antlir runtime provides a build-time sandbox for
    making other build systems reproducible.  Conceptually: you build an
    image with all your build dependencies, and then use
    `image.genrule_layer` to run a build step inside it.  For a working
    example, check out `image_rpmbuild_layer`, which is a light analog of
    [`mock`](https://github.com/rpm-software-management/mock/wiki), with the
    added benefit that *both* the build image, and the build dependencies
    are completely reproducible.
  - **Non-OS filesystems:** One can, of course, build non-OS filesystem
    images — and it is even possible to test them by `feature.layer_mount`ing
    them into a test layer that does have the OS tools that you need.
  - **Easy btrfs comparison:** Antlir comes with an elegant toolbox for
    validating the entire[^btrfs_diff] contents of btrfs filesystems.  We
    use this extensively for integration testing of image builds, for
    example, this asserts the **complete** state of an image:
    ```py
    self.assertEqual(
        ["(Dir)", {"a_dir": ["(Dir)", {"empty_file": ["(File m444)"]}]}],
        render_subvol(subvol),
    )
    ```

## What Antlir cannot (yet?) do

  - Bitwise reproducibility of artifacts — check back in late 2021 to see if
    this changes.
  - Non-RPM package managers, although eventual support for [Arch
    pacman](https://wiki.archlinux.org/index.php/pacman) is likely.  If you
    are well-positioned to contribute and maintain  `.deb` support, we are
    eager to support you.
  - A “production-identical” container runtime.  You get a container when
    you `buck {test,run}` your image, but we lack a tool to run in exactly
    the same container runtime on your favorite container manager.  Such
    deployment could be supported for limited use-cases, but “deep”
    integration is expensive — create a Github issue you want to maintain
    such an integration for your favorite runtime.  For now, you can build &
    package with Antlir, and deploy it with your production runtime, hoping
    that the setup differences are negligible.
  - Building images with tools besides Buck — though you can certainly
    ingest binaries from other build systems by wrapping them with Buck
    [`genrule`](https://buck.build/rule/genrule.html)s, or `genrule_layer`s,
    see e.g. `rpmbuild_layer` (**TODO**: link).  We can also imagine a
    partnership to integrate similar build systems, especially Bazel, whose
    macro layer leverages Starlark with a more powerful composition model.

## Footnotes

[^build_info]:  The only safe way to embed buildinfo into binariesis via
`buck --config buildinfo.timestamp="$(date +%s)"` — never shell out to
`date` as part of a build rule.

[^btrfs_diff]: `antlir/btrfs_diff` knows how to compare all the VFS features
supported by btrfs sendstream v1: special files, xattrs, cloned extents,
etc.  Caveat — v1 does not support `chattr`, but this should be added in v2
or v3.
