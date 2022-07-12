---
id: genrule-layer
title: image.genrule_layer
---

## Why you should *not* use `image.genrule_layer`

The core `image.layer` abstraction deliberately prevents the execution of
arbitrary commands as part of the image build.  There are many reasons:

  - Arbitrary commands, even with no network, can easily be
    non-deterministic (e.g. code that uses time / RNGs, or code that
    depends on the inherent entropy of the process execution model).
    Eventually, it would be nice to integrate something like
    [DetTrace](https://github.com/dettrace/dettrace), but this is out of
    scope for now.

  - We value `feature`s, which permit order-independent composition of
    independent parts of the filesystem.  For this declarative style of
    programming to work, the compiler needs to fully understand the side
    effects of evaluating a feature.

  - When executing an arbitrary command, the modified filesystem can
    arbitrarily depend on the pre-existing filesystem.  So, in order to be
    deterministic, arbitrary commands must be explicitly ordered by the
    programmer.

  - Arbitrary commands typically use shell syntax, which is both fragile, and
    is not adequately covered by `.bzl` linters. Adding more powerful linters
    is possible (e.g. ShellCheck for shell fields), but does not make shell
    scripts as obvious to the reader as intention-oriented `.bzl` programs.

## When `image.genrule_layer` *may* be appropriate

We neither can, nor should, support every possible filesystem operation as
part of `antlir/compiler` core.  This is where the "genrule layer"
abstraction comes in.

A genrule layer runs a command inside the snapshot of a parent image, and
captures the resulting filesystem as its output.  It is the `antlir` analog
of a [Buck `genrule`](https://buck.build/rule/genrule.html).  To encourage
determinism, the command has no network access.  You can make other build
artifacts available to your build as follows:

```py
export_file(name='untranslated-foo')  # or a real build rule

image.layer(
    name = '_setup_foo',
    parent_layer = '...',
    features = [
        # `genrule_layer` runs as `nobody` by default
        feature.ensure_subdirs_exist('/', 'output', user='nobody'),
        feature.install(':untranslated-foo', '/output/_temp_foo'),
    ],
)

image.genrule_layer(
    name = '_translate_foo',
    parent_layer = ':_setup_foo',
    rule_type = 'describe_what_your_rule_does',
    antlir_rule = 'user-facing',
    cmd = ['/bin/sh', '-c', 'tr a-z A-Z < /output/_temp_foo > /output/FOO'],
)

image.layer(
    name = 'foo',
    parent_layer = ':_translate_foo',  # provides '/output/FOO'
    features = [feature.remove('/output/_temp_foo')],  # clean up temporary state
)
```

Customers should not use `image.genrule_layer` directly, both because using
arbitrary commands in builds is error-prone, and because the goal is that
image build declarations be as intent-oriented as possible.

Instead, we request that library authors create self-contained, robust,
deterministic, intent-oriented abstractions on top of `image.genrule_layer`.
When the resulting rule is either a natural part of Antlir, or generically
useful, you can place it in a subdirectory of `bzl/genrule/`.  For anything
project-specific, please keep it with your project.  For a reasonable
example, take a look at `bzl/genrule/rpmbuild`.

The general idea should be to create a layer per logical image build step,
though the macro may also create intermediate layers that are not visible to
the end user.

Layering explicitly sequences the steps, and also avails us of Buck's
caching of build outputs, so that iterating on child layers does not cost a
re-build of the parent.  To make the most of caching, try to put the steps
that change most frequently later in the sequence (this parallels the best
practice for developing `Dockerfile`s).

In some cases, you are not interested in the entirety of the genrule layer,
but only in a few artifacts that were built inside of it.  The example of
`rpmbuild` works this way.  Follow that same pattern to get your files:
  - Have `image.genrule_layer` leave the desired output(s) at a known path
    in the image.
  - To use the output(s) in another image, just use regular image actions
    together with `image.source(layer=':genrule-layer', path='/out')`.
  - The moment you need to use such outputs as inputs to a regular Buck
    macro, ping `antlir` devs, and we'll provide an `image.source`
    analog that copies files out of the image via `find_built_subvol`.

## Rules of `image.genrule_layer` usage:

  - Always get a code / design review from an `antlir` maintainer.
  - Do not use in `TARGETS` / `BUCK` files directly.  Instead, define a
    `.bzl` macro named `image_<intended_action>_layer`.
  - If the macro is truly general-purpose, please put it in
    `antlir/bzl/genrule/<intended_action>`.
  - Do not change any core `antlir` code when adding a genrule layer.  If
    your genrule layer requires changes outside of `antlir/bzl/genrule`,
    discuss them with `antlir` maintainers first.
  - Tests are mandatory, see `antlir/bzl/genrule/rpmbuild` for a good
    example.
  - Keep your macro deterministic.  The Buck linters and runtime try to
    catch the very shallow issues, but here are some other things to think
    about:
      - Review our [`.bzl` coding
        conventions](contributing/coding-conventions/bzl-and-targets).
      - Avoid reading clocks or timestamps from the filesystem, or local
        user / group IDs, or other things that can be different between your
        dev host, and another host.

## Deliberate limitations of the `image.genrule_layer` implementation

  - No network access. Network builds are a gateway to non-deterministic hell.
    If you're sure your use-case is "safe", talk to `antlir` maintainers
    for how to implement it correctly (repo-committed checksums, etc).

  - We will not add `--bindmount-{ro,rw}` to the container invocation.
    Normal `feature.layer_mount`s in the parent will, of course, work as
    intended, but these are not meant to let you bind-mount arbitrary host
    paths, and so ought not to lead to non-determinism. As in the example
    above, `image.install` is another good way to get data into your image.

    Details on the rationale: The only paths that are safe to bind-mount
    into a container are `buck-out` build artifacts. Previously mentioned
    `image.{install,layer_mount}` should adequately address this. Doing
    runtime mounts would be less deterministic because:
      - the tree being bind-mounted will have nondeterministic `stat` metadata.
      - `nspawn` bind-mounts leave behind in the image an implicitly created
         set of dirs and files for the mountpoint, and the `stat` metadata
         for these won't be deterministic either.

## Ok, I will be very careful &mdash; where are the API docs?

The [regular API docs](api/bzl/image.bzl.md#genrule_layer) describe the function arguments.
