---
id: vision-containers-as-build-artifacts
title: "Vision: Containers as Build Artifacts"
---

# NB: This was written in H1 2019, but the system has kept evolving.

The high-level overview is still good, and the conceptual structure still
stands. However, you might see some variation in syntax or naming. If you spot
these, please fix this wiki, or ping @lesha.

## tl;dr

Today, you can `buck build` and `buck test` custom filesystems for Tupperware
containers (“What works” below). Buck improves the workflow for a few teams, but
we want to bring the same “test early, test often, deploy safely” methodology to
every Tupperware service. We are hiring — see e.g. hackamonths
[T46795296](https://our.intern.facebook.com/intern/tasks/?t=46795296),
[T46832242](https://our.intern.facebook.com/intern/tasks/?t=46832242),
[T46837515](https://our.intern.facebook.com/intern/tasks/?t=46837515)
(["buck-service-containers" task tag](https://fburl.com/tasks/300prf3l)).

## Vision

FBCode provides smooth development, testing, and continuous integration for
program binaries. Engineers can be productive with just their editor,
`buck build`, and `buck test`. Diff reviewers see clear signal when code changes
break tests.

In contrast, defining and testing a service container is currently a complex
process, involving several additional tools (at a minimum, `configerator` with
`fbpkg` followed by `.tw` edits and `tw sandbox`). Shipping the service requires
Conveyor, Service Foundry, and some "test in production" elbow grease. Compared
to pure binaries, the iteration time is slower, the debugging experience more
esoteric, and the risk to production is often non-zero.

Our goal is to make developing service containers be just as easy and safe as
building binaries is today. Here are our core values:

- A deployable container feels like a regular build artifact: it is
  deterministically reproducible from an `hg` hash, its bugs are bisectable,
  its changelogs are human-readable. You can inspect the built container right
  in your FBCode repo.
- "What you test is what you run" — the container you inspect and test in your
  FBCode repo can be deployed to production, unmodified.
- `buck build` & `buck test` work as you expect.
- Packaging containers for deployment is *managed*, not ad-hoc — service
  owners are shielded from the details of how their build artifact gets to
  production. In return, they get transparent improvements to deployment
  efficiency (ask @mpawlowski, we have a long pipeline of perf boosts in the
  works).
- The filesystem construction language is (i) declarative — the compiler
  checks filesystem actions for compatibility, and sorts them automatically,
  and (ii) strict — to the extent possible, we enforce that actions succeed
  fully (i.e. no implicit overwriting, no accepting pre-existing stuff at the
  same location), and we do not add features that do not compose predictably
  with others.
- Follow the Unix philosophy. Our tools must integrate well with today's
  infra, but these integrations must not complicate our core concerns, or it
  will be hard to build the infra of tomorrow. To stay lean, we take the time
  to add each integration through the composition of separable, single-purpose
  tools. As a bonus, this keeps our
  [nascent open-source release](https://github.com/facebookincubator/fs_image)
  within reach.

Here is an **aspirational** `TARGETS` file defining a service, from binary
through testing & packaging:

    cpp_library(
      name = "banana_server_lib",
      srcs = ["BananaServer.cpp"],
    )

    cpp_binary(
      name = "banana-server",
      srcs = ["banana_main.cpp"],
      deps = [":banana_server_lib"],
    )

    tw.service(
      name = "banana",
      binary = ":banana-server",
      args = ["--ice=cream", "-vvv", "smoothie"],
    )

    # Implicitly depends on :banana
    tw.packager(name = "tupperware.service.banana")

    tw.python_service_test(
      name = "banana-service-test",
      service = ":banana",
      srcs = ["test_banana_service.py"],
      needed_coverage = [(100, ":banana_server_lib")],
    )

With the above `fbcode/banana/TARGETS`, a typical workflow would involve
iterating on tests before putting up a diff. This works as you expect:

    buck test //banana:banana-service-test

Your test runs inside an ephemeral container. The test code itself looks looks
exactly like a regular `python_unittest`. Tests in other languages will be
supported as well.

To make a deployable artifact, you would:

    buck run //banana:tupperware.service.banana

This will print a deployable handle, which you (or Conveyor + SF) can include in
your `.tw` spec. If you are familiar with the traditional `.tw` syntax, using
the deployable handle will replace a bunch of fields in that `.tw` spec (e.g.
`packages`, `command`, `arguments`, `pre_run_steps`). What remains in the `.tw`
file focuses on scheduling & allocation, while the `TARGETS` file now specifies
how to start & stop a single service task.

In other words, this deployable artifact knows how to reconstruct your
filesystem, and how to start your service inside it.

Normally, you would not manually package your service. Instead, you would add a
line of this sort to your
[contbuild config](https://our.intern.facebook.com/intern/wiki/Fbcode_Continuous_Build/).

    "fbpkg_builders": ["//banana:tupperware.meta.banana.service"],

This will ensure that clean, unit-tested packages of your service are published
periodically. As with regular binaries, you can then feed these packages into
Conveyor/SF for automated deployment.

## What works as of H1 2019

The toolchain has production-ready support for publishing **custom btrfs
images**. A handful of high-profile teams, including Traffic Infrastructure, Web
Foundation, and Python Foundation have migrated their images from the legacy
system, and reported no negative experiences.

### Try it for yourself

    $ cd ~/fbcode

    # Enter a built container.
    # (implicitly builds //tupperware/image/python_foundation:banderwrapper)
    $ buck run //tupperware/image/python_foundation:banderwrapper-container
    ...
    bash-4.4$ ls
    bin   data  etc   lib    logs   meta  opt       proc  run   srv  tmp  var
    boot  dev   home  lib64  media  mnt   packages  root  sbin  sys  usr
    bash-4.4$ exit

    # Execute some tests inside the container.
    $ buck test //tupperware/image/python_foundation:banderwrapper-test
    ...
    Summary (total time 2.23s):
      PASS: 3
      ...

And this
[line in the contbuild config](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/contbuild/configs/tupperware_image_python_foundation;d691e6cbafb2811750d2c161dbbd525af3956d91$8)
causes the custom image to be
[packaged continuously](https://our.intern.facebook.com/intern/sandcastle/projecthealth/?search_keywords[0]=tupperware_image_python_foundation&search_keywords[1]=testinfra_endtoend_automation&types[0]=master),
as long as tests pass. Notably, the customer did not need to interact with
Configerator or fbpkg to achieve this — all their code is in
[tupperware/image/python_foundation](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/tupperware/image/python_foundation/).

### Documentation

For each implemented feature, the code contains detailed comments explaining how
to use it. For example, almost 50% of the implementation of
[image.layer](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/fs_image/buck/image_layer.bzl)
is documentation. Links to relevant docs are sprinkled throughout the rest of
this note. It is on our roadmap to consolidate key user-facing docs on the wiki.
For now, start here and read the comments for the details.

### Naming conventions

**Image definitions:** By convention, all Tupperware custom images produced
using this toolchain live in
[fbcode/tupperware/image/](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/tupperware/image)
`<oncall_name>` (optionally, with subdirectories per project).

**Contbuild configs:** Images are built and
[tested](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/fs_image/buck/image_python_unittest.bzl)
by
[Contbuild](https://our.intern.facebook.com/intern/wiki/Fbcode_Continuous_Build).
The corresponding contbuild configs are named
[fbcode/contbuild/configs/](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/contbuild/configs/)
`tupperware_image_<oncall_name>`.

### Examples

These teams are highlighted because their images run in real clusters, and
because their use-cases demonstrate different features of the system.

**Web & Intern Foundation custom images:** The file
[tupperware/image/webfoundation/TARGETS](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/tupperware/image/webfoundation/TARGETS)
defines a `facebook.com` image. Then, the image
`//tupperware/image/intern:intern_foundation.intern`
**[inherits](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/tupperware/image/intern/TARGETS;2ea3c6f8eba19345d90b5e30efb5e8624ac01a7d$22)**
the content of `facebook.com`, and adds a few more items. You may notice that
`facebook.com` defines two different package variations —
`tupperware.image.facebook.com` and `tupperware.sendstream.facebook.com`, which
implicitly depend on the `image.layer`. There are two because Web Foundation is
trying our a newer, more efficient deployment mechanism. However, normal custom
image users should only define `tupperware.image.LAYER_NAME`, and wait for the
Tupperware team to transparently migrate everyone to the new technology when
ready.

**Python Foundation custom image:** Inside
[tupperware/image/python_foundation/TARGETS](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/tupperware/image/python_foundation/TARGETS),
you will find a `image.python_unittest` target. This accepts all usual
`python_unittest` arguments — and it's not just for Python services, refer to
the "`TARGETS` rule types" section below.

**TI Proxygen custom image:** The image in
[tupperware/image/ti_proxygen/TARGETS](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/tupperware/image/ti_proxygen/TARGETS)
shows a neat pattern that was impossible before Buck images. On the legacy image
build system, the Proxygen image would copy a rarely rebuilt,
push4push-unfriendly `fb-drip` fbpkg into the image. In the current image, a
fresh, built-from-trunk `drip` is copied directly into the image. Since Proxygen
has automated canary and deployment, their pre-push tests automatically validate
that the new binary works with the new service ... and now this un-owned binary
is always up-to-date. **Important:** Consult with @lesha or @lsalis before doing
this for your service — today's `fbpkg_builder` has some rough edges that may
mean you want to wait until H2 2019.

### Core infrastructure

-   We built a toolbox for determinstically constructing
    [btrfs](https://btrfs.wiki.kernel.org/index.php/Main_Page) filesystem
    images, and for analyzing btrfs send-streams. Btrfs snapshots enable us to
    efficiently layer multiple build steps.
-   The btrfs toolbox is well-integrated with Buck, which is non-trivial since
    Buck itself can only manage rule outputs that are plain old files — not
    complete filesystems with rich Linux metadata. Unused btrfs subvolumes are
    garbage-collected so your devbox does not run out of space.
-   All our code is testable by design, with enforced 100% test coverage. We
    invested in making it easy to write expressive tests — this
    [handful of lines asserts](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/fs_image/compiler/tests/test_items.py;65cc006228fb8d852d70b5cc53132319b5bd71d4$259-270)
    the complete state of the filesystem, down to SELinux attributes, xattrs,
    and even btrfs cloned blocks.
-   We periodically store immutable snapshots of all prod RPM repos to make RPM
    installation fully deterministic. Container images frequently install RPMs
    to deliver dependencies. Our RPM repos are extremely dynamic, making the
    behavior of `yum` vary from moment-to-moment and even host-to-host. Despite
    this, Buck build artifacts **must** be fully reproducible for many reasons,
    including security, bisects, and artifact caching. Efficient repo snapshots
    allow us to deterministically provide up-to-date software.
-   The tools for building filesystem come with an integrated Linux container
    runtime, which powers `buck run` and `buck test`. A container is more than a
    disk image — booting a Tupperware job involves setting up mounts, cgroups,
    namespaces, and a host of other non-persistent state in the kernel and
    userland. The Buck runtime is currently a lightweight approximation of TW
    agent, but there are plans to use the production agent for build-time
    testing.
-   [partly rolled out] The image build environment is a reproducible, immutable
    OS that is independent of the host OS, and is optimized for quickly
    installing the in-fbcode RPM snapshot.

### `TARGETS` rule types

While the higher-level `tw.service` rule is still at the concept stage, we have
lower-level production-ready Buck rules that will power the TW-specific syntax
sugar from the vision above. The code has detailed API documentation —
typically, you will want to read the file doc-block, and then skip to the `def`
of the main function:

- [image.layer](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/fs_image/buck/image_layer.bzl):
  A container filesystem that acts as a Buck artifact. Layers support
  inheritance, and can be mounted inside other layers.
- [image.feature](https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/fs_image/buck/image_actions/feature.bzl):
  A library-like abstraction — a set of things to be done to any layer
  that includes this feature.
-[image.python_unittest](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/fs_image/buck/image_python_unittest.bzl):
  A regular `python_unittest` that runs inside an `image.layer`. This lets you
  you validate your filesystem by running code inside a container that loosely
  approximates production. You are encouraged to use the
  [needs_coverage assertion](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/fs_image/buck/tests/test_image_python_unittest.py)
  to ensure 100% of your service's library gets exercised within the
  container. If your service is in C++ (or Java or Rust), you can easily
  include the `{cpp,java,rust,...}_binary` in the `resources` of your
  `image.python_unittest`, and use the Python test to exercise the binary.
  Support for `image.<yourlanguage>_unittest` will be added as requested.
- [tw.image_fbpkg_builder](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/tupperware/image/buck/tw.bzl):
  The TW-managed solution for packaging and distributing custom btrfs images.
  Customers write a one-line target, which knows how to publish an
  `image.layer` from the same `TARGETS` file. Adding this target to the
  ["fbpkg_builders" field of the contbuild config](https://our.intern.facebook.com/intern/wiki/Fbcode_Continuous_Build/config-file-reference/)
  will publish contbuild-tested ephemeral fbpkgs, ready for your
  [continuous deployment pipeline](https://our.intern.facebook.com/intern/wiki/One_World/Infrastructure/Service_Foundry/).
  This rule is a simple combination of two primitives below — but prefer to
  use the higher-level one, since TW has specific plans to (transparently)
  optimize the packaging and distribution of custom images.
  - [image.package](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/fs_image/buck/image_package.bzl):
    a serialization primitive for layers
  - [fbpkg.builder](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/fs_image/buck/facebook/fbpkg_builder.bzl):
    a way to define and (cont)build Fbpkgs straight fbcode, no Configerator config needed.
- [fbpkg.fetched\_layer](https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/fs_image/fbpkg/facebook/fbpkg.bzl):
  *(ready for use in container tests, but not for production)* A simple way to
  mount an `fbpkg:tag` in your container — add a new package by running
  `buck run fs_image/fbpkg/facebook/db:update-db -- --db fs_image/fbpkg/facebook/db/main_db.bzl --create YOUR_PKG YOUR_TAG '{}'`,
  and refer to the new target in your layer via in the `mounts` field. Let's
  contrast this with specifying `fbpkg:tag` in the `packages` field of your TW
  job. At present, if the `tag` changes mid-way through your ServiceFoundry
  push, your job spec will be considered changed, and SF may re-push the
  entire job (this has caused Proxygen SEVs). In contrast, if you build a
  container with an `fbpkg.fetched_layer`, the `tag` is resolved to a UUID at
  build time, so the job that you deploy will always be the one that you
  tested, even if the `tag` moves ahead. Mounting an `fbpkg.fetched_layer` is
  also much better than\* copying\* the fbpkg into your custom image. If you
  copy N fbpkgs of M bytes into your image, each image update will re-deploy N
  \* M bytes even if only M of them changed. In other words, by copying into
  the image you will incur more I/O per update, your updates will be slower,
  and you will rob TW agent of the ability to share an fbpkg between multiple
  containers on the host.

### Inheritance and composition

Containers often add significant complexity beyond their constituent binaries —
common concerns include installing dependencies, describing the correct start-up
of processes, and orchestrating the relationships between binaries and
processes. To manage this complexity, it is most appropriate to engineer the
container as any other software artifact: to be assembled in from modular units,
with each unit tested separately, and with additional tests for the integration.

This section covers the supported techniques for modularizing filesystem
construction. The above `TARGETS` rule types allow you to use the traditional
means of composition:

-   **Primitives:** The system must provide some actions to compose. Such
    primitives include making directories & symlinks, copying outputs of other
    build rules (and repo files via the `export_file` rule), and even extracting
    deterministic tarballs (for fbpkg support). See `def image_feature` in
    [image\_feature.bzl](https://phabricator.intern.facebook.com/diffusion/FBS/browse/master/fbcode/fs_image/buck/image_feature.bzl)
    for an up-to-date list.
-   **Inheritance:** By specifying `parent_layer` in `image.layer`, the new
    layer inherits the entire contents and runtime configuration of its parent.
    Thanks to btrfs snapshots, layer inheritance is very fast. Just like in
    `docker build`, Buck layer caching can drastically improve the speed of
    iterating on the top layers. Unlike Docker, though, the layers are truly
    hermetic, and are guaranteed to be rebuilt when the inputs do change. As
    with regular code, inheritance must be used sparingly. First, it is risky —
    the child receives all future modifications to the parent. Second, it is
    often inappropriate — how often do two artifacts truly satisfy an "is a"
    relationship?
-   **Composition of actions:** An `image.feature` defines how to manipulate the
    `image.layer` that includes it. The feature’s actions are not materialized
    eagerly, so this style of composition incurs additional build-time work each
    time a layer includes a feature. Worse yet, each dependent layer will pay a
    separate I/O cost for distributing similar sets of bits produced by those
    actions. In exchange for these expenses, features bring flexibility, since a
    feature's behavior can adapt itself to the contents of the layer under
    construction. It's important to note that features (and their constituent
    actions) deliberately have no way of customizing the order of the actions,
    so any actions that can be defined by features must commute, or be subject
    to a well-defined implicit ordering.
-   **Composition of subtrees (“mounts”):** Thanks to btrfs snapshots,
    `image.layer` inheritance is performant. However, it can be risky or
    inappropriate. Action composition is flexible, but results in duplicate work
    for builds & artifact distribution. Composition of subtrees aims to provide
    a happy medium. If your filesystem feature can be defined entirely by one
    subtree, like `/opt/foo_application`, you are in luck. In this case, the
    feature can be assembled as its own `image.layer` and mounted into other
    layers via `mounts = {"/opt/foo_application": "//path/to:foo_app_layer"}`.
    Then, `foo_app_layer` will get built just once, no matter how many other
    layers mount it. Its bits will get packaged only once for distribution. A
    host running containers that include `foo_app_layer` will download just one
    copy, and that copy will be mounted into all containers that use it. An
    important case of subtree composition is mounting an `fbpkg.fetched_layer`
    into your layer. This is much more efficient than copying an fbpkg into a
    custom image.

## What remains to be done

### Task list

For the time being, we're consolidating all Buck-related tasks in this document:
[Work items for service containers as build-time artifacts (aka Buck image build)](https://fb.quip.com/YR5sAUGA74lc)
— we'll probably switch to Tasks eventually, but a flat file is better for now.

### Buck is not just for custom images

The "Vision" section above shows how `TARGETS` files will be used to describe a
service, but it makes no mention of a custom image — i.e. an entire filesystem
that is materialized and packaged opaquely at build-time.

Let’s contrast custom images to the 2018 default for Tupperware services:

- TW provides the entire filesystem,
- an fbpkg containing just the fbcode-platform-based service binary is
  bind-mounted on top of that filesystem.

This 2-step default has low costs (at runtime, in maintenance, and in
distribution), but it does not permit arbitrary changes to the container images.
Hence, some teams find custom images to be necessary.

A growing number of customers are using `TARGETS` to build custom images. The
eventual goal is to allow TARGETS files to also be used by the typical TW
customer to specify their filesystem in the "base image + bind-mount" style.

Migrating all filesystem details to `TARGETS` gives two big wins:

- All TW container filesystems become testable at diff-time.
- The distribution of the container filesystem becomes a (mostly opaque)
  implementation detail, which frees the TW team to dramatically optimize
  container filesystem delivery in practice.

There are a few lower-level "to-do"s necessary to support non-custom images from
`TARGETS` files. These are detailed in the task list.

Beyond `TARGETS`-defined filesystems, it is part of the vision to also be able
to build via `TARGETS` much of the service runtime configuration.
[This group post](https://fb.prod.facebook.com/groups/btrmeup/permalink/2147662275313427/)
contains a rough sketch of the eventual implementation.

An attentive reader will notice that the idea of configuring TW jobs at
build-time overlaps with Hermetic Configs, and the testing feature set resembles
a watered-down Cogwheel. Luckily, we are in touch with both teams, and feel that
we are pushing towards a shared broader vision. Comment for a more discussion of
how these technologies relate.
