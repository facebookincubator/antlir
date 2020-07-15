---
id: bzl-and-targets
title: .bzl and TARGETS
---

## Stay lint clean

Enough said. Critically, this ensures that we don't stray outside of the
restricted feature-set of the Starlark language (the Buck runtime is
currently much more permissive).

## Please maintain `fake_macro_library` dependencies

Take a look at the doc in
[fs_image/bzl/TARGETS](https://www.internalfb.com/intern/diffusion/FBS/browse/master/fbcode/fs_image/bzl/TARGETS?lines=5).
This is kind of a chore, but it helps kick off the right CI jobs when we edit
`.bzl` files, so it's worth doing.

Ideally, we would just write a linter to do this on our behalf. However,
we haven't yet found time.

*Note:* The vmtest macros have not yet been updated to follow this pattern, help
is welcome!

## Target naming: dash-separated binaries & layers, underscore-separated libraries & features

This convention follows `fbcode/folly/`. One concrete benefit is that it's
easier to spot when a `python_binary` is being used as a library without the
`-library` suffix to reference the implicit library target.

## Write pure functions, macros, or macro wrappers

The failure mode here is writing something that is neither clearly a
function nor a macro, but a mix.

-   A function defines no targets, returns a value, and has no side-effects.
    Functions that take mutable arguments are acceptable only in very limited
    circumstances (e.g.
    [set_new_key](https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/fs_image/common.py?commit=73c7b3f113146faebd6133d42eaf751cd05d9a8c&lines=77-81)).
-   A macro takes `name` as its first arg, and defines a target of that name
    (along with possibly auxiliary functions). When convenient, an internal
    macro **may** return a path to the target it created, but we have not made
    this the norm for externally visible macros.
-   A macro wrapper transforms a target into a wrapped target, and returns the
    path to the wrapper. You should write these very rarely.

## No mutable state outside of functions

If you define a module-level `a = []`, and mutate it from your macros, this is a
sure-fire way to get non-deterministic builds. This kind of thing has actually
caused subtle breakages in the FBCode Target determinator before, requiring
multiple human-days to find and fix.

## Do not expose magic target names to the user

If your macro defines an purely internal target, make sure it's namespaced so
that, ideally: - It does not show up in `buck` TAB-completion (put your magic in
the prefix, not suffix) - The magic prefix should discourages people from typing
it manually into their TARGETS files or `.bzl` files -- provide an accessor
method when this is necessary, see e.g.
[fbpkg.fetched_layer](https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/fs_image/fbpkg/facebook/fbpkg.bzl)
- If appropriate, use
  [mangle_target](https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/fs_image/bzl/target_tagger.bzl?commit=30ea8293608c719e3dc2ccdaaa3e6a2acc234265&lines=74),
  but be aware that it's currently possible for the mangled form to collide
  among different input target names (there are lo-pri work items for this).

There are exceptions to this, which are magic target names that we expect users
to type as part of a `buck` command-line on a regular basis, e.g.: -
`-container` and `-boot` for `image.layer`s -`--test-layer` for
`image.*_unittest`s

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
binary_path=( $(exe //fs_image:artifacts-dir) )
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

## Use `maybe_export_file` when appropriate

If your macro takes an argument that is a target, and that target might
sometimes be an in-repo file, use `maybe_export_file
<https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/fs_image/bzl/maybe_export_file.bzl>`__.

Use ``fs_image_internal_rule`` for any internal rules
-----------------------------------------------------

If you're using any macros to generate internal "wrapper" rules (i.e. any rule
that doesn't use the ``name`` of the underlying target), you should set the
``fs_image_internal_rule`` kwarg to true in the definition.

This is good practice as it enables further post-processing when determining
dependencies. We use this internally in fb for example to compute the
"human-visible" dependency cost to a target, after omitting internal targets.
