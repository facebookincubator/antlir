---
id: why-build-containers-using-buck
title: Why Build Containers Using Buck?
---

## Reproducibility

Our goal is **hermetic** builds. Specifically, that means that given an
`fbsource` revision, `buck build path/to/image:target` will produce a
**semantically identical** output every time, whether you run the build today,
or your intern runs it 1 year later. "Semantically identical" is weasel wording
to acknowledge the fact that `buildinfo` and other such metadata will differ
between the two artifacts, so the outputs will not be bit-identical. However,
they build artifacts should **work** the same way.

Buck is designed around hermetic builds. The output of each of its rules can
only depend on the inputs, and the inputs themselves are guaranteed to be
functions of the current repo rev, and nothing else. Buck provides some training
wheels & validation to make sure that build rules are hermetic, and is planning
to add a lot more (as per my conversation with @pjameson). By hooking into Buck,
we will benefit from most of the team's sandboxing work.

## Benefit: Services have predictable filesystems (WYSIWIG)

With images, a service's filesystem state is frozen at `buck build` time, not at
Tupperware staging time. You can inspect the built filesystem, test it, analyze
it, archive it, and only then deploy it.

This approach is far more what-you-see-is-what-you-get than the current practice
of finalizing the filesystem milliseconds before running your service in prod
(or in a canary, if your team follows current best practices).

As a result, most services should be able to reap tangible reliability and
debuggability improvements by adopting images.

## Benefit: Reverts that work

This is a place-holder, but
[Hermetic_Configs](https://www.internalfb.com/intern/wiki/Hermetic_Configs/) has
an excellent description of The Revert Problem™.

## Benefit: Bisects at the service level

Imagine running `hg bisect` and finding that your service was broken by an RPM
version upgrade, or by a change to a host-level service like smcproxy. Hermetic
filesystem builds will make it possible to catch more failures early.

## Benefit: Continuous integration has fewer differences from production

Hermetic builds let us run continuous integration on the build artifacts. In
other words, images will get all the benefits of Sandcastle & contbuild. Here
are a couple of high-value features many teams will want:
- Run tests on diffs: if your image build is affected by a change in fbcode, it
  will be re-built, and all of its functional tests will be run. In other words,
  all teams will get automatic integration testing with minimal effort.
- For images whose automated tests pass, automatically feed them through
  Conveyor for graduated canarying aka "testing in prod".
- Auto-deploy auto-tested images: teams often use continuous integration
  artifacts that pass tests as release candidates. By building images instead of
  binaries, we significantly reduce the risk that the binary will break in prod,
  because the image is more self-contained.

## Benefit: Familiarity

Every service owner knows how to `buck build` and `buck test` targets. By making
images work as binaries, we will make them much easier to adopt.

## Benefit: Faster staging on Tupperware

With old-style Tupperware jobs, a task could sit in a "STAGING" state for
minutes or more while it downloaded, and installed dozens of packages. This
results in unnecessarily long downtime for services, since Tupperware must not
stage a task while its prior version is running (the reason: staging runs
arbitrary code! the task is in a sense "up" when it stages). Worse yet, the
package installation would sometimes be non-deterministic, break due to external
dependencies, or even corrupt the underlying host. With images, what you see at
build-time is what you get in production — and your downtime is minimal because
the entire image can be pre-fetched and unpacked before your task starts.

## Unformatted notes about the impact of image-style service distribution on users

-   Easier to reason about the totality of dependencies. Inspectable, freezable,
    etc.
-   More complex execution model than plain binaries (some runtime is involved).
    `buck run` can hide this in the common case, but power users still have to
    learn a docker- or systemd-type API for interacting with such services.
-   Much bigger binaries (for the time being — optimizations are possible but
    not prioritized).
-   Perfect reverts are possible (you CAN revert all on-disk dependencies,
    including RPM repos, and fbpkgs). Of course, Configerator and other runtime
    state still remains a one-way sausage-maker.
-   Continuous integration that works better / gives more signal (integration
    tests, breakage via dependencies, automated test-in-prod aka canary).

## Tasks

As of September 2018, most of the outstanding tasks are not in the Tasks tool,
but in [this quip](https://fb.quip.com/YR5sAUGA74lc).
