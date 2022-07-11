---
id: bzl-and-targets
title: .bzl and TARGETS
---

## Stay lint clean

Enough said. Critically, this ensures that we don't stray outside of the
restricted feature-set of the Starlark language (the Buck runtime is
currently much more permissive).

## Target naming: dash-separated binaries & layers, underscore-separated libraries & features

This convention follows `fbcode/folly/`. One concrete benefit is that it's
easier to spot when a `python_binary` is being used as a library without the
`-library` suffix to reference the implicit library target.

## Write pure functions, macros, or macro wrappers

The failure mode here is writing something that is neither clearly a
function nor a macro, but a mix.

-   A function defines no targets, returns a value, and has no side-effects.
    Functions that take mutable arguments are acceptable only in very limited
    circumstances (e.g. [set_new_key](
    https://github.com/facebookincubator/antlir/blob/master/antlir/common.py#L69
    )).
-   A macro takes `name` as its first arg, and defines a target of that name
    (along with possibly auxiliary functions). When convenient, an internal
    macro **may** return a path to the target it created, but we have not made
    this the norm for externally visible macros.
-   A macro wrapper transforms a target into a wrapped target, and returns the
    path to the wrapper. You should write these very rarely.

## No mutable state outside of functions

If you define a module-level `a = []`, and mutate it from your macros, this
is a sure-fire way to get non-deterministic builds.

The precise reason is that Buck doesn't guarantee order of evaluation of
your macros across files, so a macro that updates order-sensitive mutable
globals can create non-determinism that breaks target determinators for the
entire repo, potentially costing many human-days to triage & fix.

## Be careful with traversal ordering

If you're not sure whether some container or traversal is guaranteed to be
deterministically ordered in Buck, sort it (or check).

## Stay Starlark-compatible

Keep in mind that Buck currently supports at least two frontends for `.bzl`
files: python3 and Starlark (and the default differs between FB-internal and
open-source).  You must write code that is compatible with both.

To check both back-ends, run:

```
buck targets -c parser.default_build_file_syntax=skylark //your/proj:
buck targets -c parser.default_build_file_syntax=python_dsl //your/proj:
```

# Start failure messages with `AntlirUserError:` when appropriate

This prefix is not necessary when you `fail()` from `.bzl`, since such
errors are readily visible to the user.

However, if you have a genrule that checks for user errors, and writes to
stderr, it is important to prefix your error message with `AntlirUserError:`.
If necessary, emit a newline to make sure that the "A" is first on the line.

This will trigger CI automation to highlight this error in the logs.

## Beware `cacheable = False`

A number of Antlir rules need to be marked `cacheable = False` for various
reasons. Avoid this feature if at all possible:

  - Any cacheable rule that includes the output of the uncacheable rule will
    (almost) necessarily encounter caching bugs. Concretely, imagine that
    `:foo` is an uncacheable genrule, and it is included in the `resources`
    of a `python_binary(name = "bar")`. Then, production CI will end up
    caching `:bar` together with the now-invalid contents of `:foo`, and
    this will cause bugs in your builds and/or tests, potentially subtle
    ones.

  - Uncacheable rules hurt build performance since they cannot be fetched
    from distributed caches.

## Do not expose magic target names to the user

If your macro defines a purely internal target, make sure it's namespaced so
that, ideally: - It does not show up in `buck` TAB-completion (put your magic in
the prefix, not suffix) - The magic prefix should discourages people from typing
it manually into their TARGETS files or `.bzl` files -- provide an accessor
method when this is necessary, see e.g.  the FB-internal `fetched_layer` in
`fbpkg.bzl`.

  - If appropriate, use [mangle_target](
    https://github.com/facebookincubator/antlir/blob/master/antlir/bzl/target_helpers.bzl#L32
    ).

There are exceptions to this, which are magic target names that we expect users
to type as part of a `buck` command-line on a regular basis. Reference [Helper Buck Targets](../../tutorials/helper-buck-targets.md) for a list of examples.

## Get expert review when writing genrules

There are a lot of failure-modes here, from quoting to error-handling, to
mis-uses of command substitution via `\$()`, to mis-uses of `$(exe)` vs
`$(location)`, to errors in cacheability. For now, treat any diff with such code
as blocked on a review from @lesha. We need a second domain expert ASAP.

To get a taste of some potential problems, carefully study
`_wrap_bash_build_in_common_boilerplate` and
`maybe_wrap_runtime_deps_as_build_time_deps`. This is not exhaustive.

## In genrules, use bash arrays for `$()` command substitution

You know what `"$(ls)"` does in `bash`. Now you want this in the `bash =` field
of your genrule. Unfortunately, this is hard. You have to do this two-liner:

```
binary_path=( $(exe //antlir:artifacts-dir) )
artifacts_dir=\\$( "${binary_path[@]}" )
```

Understanding what follows starts with carefully reading the
[genrule docs](https://buck.build/rule/genrule.html).

You have to use `exe` instead of `location` because the latter will rebuild your
genrule if the **runtime dependencies** of the executable target change, while
the former will only rebuild if the **content** of the executable change.
Specifically, in @mode/dev, if the executable is a PAR, its content is just a
symlink, which never changes, so your genrule never rebuilds. Even with C++, you
would fail to rebuild on changes to any libraries that are linked into your
code, since in `@mode/dev` those are `.so`s that are not part of the target's
"content".

You have to use a bash array because `$(exe)` expands to multiple shell words,
Because Buck (TM). E.g. for PARs, the expansion of `$(exe)` might look like
something like `python3 "/path to/the actual/binary"`.

## In genrules, prefer `out = "out"`

The `out` field is not user-visible, it is just an implementation detail of
the filesystem layout under `buck-out`.  As such, its value does not matter.
Unfortunately, Buck requires it.  To minimize cognitive overhead and naming
discussions, we prefer for it to always say `out = "out"`.  Feel free to
update legacy callsites as you find them -- there is no risk.

## Use `maybe_export_file` when appropriate

If your macro takes an argument that is a target, and that target might
sometimes be an in-repo file, use [maybe_export_file](
https://github.com/facebookincubator/antlir/blob/master/antlir/bzl/maybe_export_file.bzl
).

## Load from `oss_shim.bzl`, avoid built-in (or fbcode) build rules

This shim exists to bridge the differences between the semantics of
FB-internal build rules, and those of OSS Buck.  If you bypass it, you will
either break Antlir for FB-internal users, or for OSS users.

Note that any newly shimmed rules have to follow a few basic practices:
 - Follow the fbcode API, unless the rule has no counterpart in fbcode.
 - Add both an OSS and FB implementation.
 - In both implementations, wrap your rule with `_wrap_internal`.
 - Follow the local naming & sorting conventions.

## Mark user-instantiated rules with `antlir_rule = "user-{facing,internal}"`

All Buck rules used within Antlir have an `antlir_rule` kwarg.

You can declare Buck rules in one of three contexts.  The context
corresponds to the value of the `antlir_rule` kwarg:

 - `"antlir-private"` (default): A private implementation detail of Antlir --
   e.g.  a `python_library` that is linked into the image compiler.  These
   rules need no explicit annotation.

 - `"user-facing"`: A rule that may be instantiated in a user project (aka
   a Buck package outside of `//antlir`), and whose output is directly
   consumed by the user.  Specifically, the rule's `name` must be the `name`
   provided by the end-user, and the artifact must be user-exposed.  For
   example, `package.new` is user-facing, whereas `feature`s or
   `image.layer` are considered implementation plumbing, even though users
   declare them directly.

 - `"user-internal"`: A rule that may be instantiated in a user project,
   whose output is not directly usable by the client.  Besides
   `image.{feature,layer}`, this includes private intermediate targets like
   `PREFIX-<name>`.

**Marking rules `"user-internal"` is important**, since FB on-diff CI only
runs builds & test within a certain dependency distance from the modified
sources, and `"user-internal"` targets get excluded from this distance
calculation to ensure that the right CI targets get triggered.

To ensure that all user-instantiated (`"user-facing"` / `"user-internal"`)
rules are annotated, un-annotated rules will **fail to instantiate** from
inside a user project.  That is, if your rule doesn't set `antlir_rule`, it
defaults to `"antlir-private"`, which triggers `_assert_package()`, which
will fail if the Buck package path does not start with `antlir/`. This
has two desirable effects:
 - Antlir devs will not forget to annotate user-instantiated rules.
 - External devs will not be able to (erroneously) load rules from
   `oss_shim.bzl`.

The implementation details and more specific docs can be found in
`antlir/bzl/oss_shim_impl.bzl`.

## Naming conventions when using `shape.bzl`

Shape types should be named with a trailing `_t` to indicate that it is a
shape type. Shape instance variable names should conform to the local style
conventions.

For example, the type and instance for installing a tarball might look like
this:
```
tarball_t = shape.shape(
  from_target = shape.field(str, optional = True),
  into_dir = str,
)

install_tarball = shape.new(tarball_t, from_target=..., into_dir=...)
```
