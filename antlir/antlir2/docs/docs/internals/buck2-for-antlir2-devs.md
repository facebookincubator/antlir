---
sidebar_position: 1
---

# buck2 for antlir2 devs

The full buck2 docs are available [here](https://buck2.build/), but this page
will endeavour to teach you just enough buck2 terminology and how it relates to
antlir2 to be useful.

## Glossary

Some quick definitions of terms that we use throughout the codebase, technical
conversations and anywhere else.

### Starlark

The simple, Python-like language of buck2, buck, bazel, and several other build
systems.

### Buildfile

Often referred to as just "`TARGETS` files", these has a few alternative names
(`BUCK`, `TARGETS.v2` and `BUCK.v2`)

(The `.v2` suffix means that that version of the file should be loaded on buck2,
and the non-suffixed file [if any] is loaded by buck1)

A buildfile is where all [targets](#target) are defined.

### .bzl file

A `.bzl` file is written in [starlark](#starlark) and allows users or antlir2
developers to create [targets](#target) more dynamically.

A `.bzl` file can contain regular old Python-like starlark functions (that
expand to [rule](#rule) calls, other macros, or just contain pure logic),
variable definitions or [rule](#rule) implementations.

### Target

Simply speaking, this is anything that shows up in `buck2 targets` command.

A target is the entrypoint for users to build [things](#artifact). In buck2,
targets are especially interesting because they come with
[providers](#provider).

#### Target Label

Targets have a canonical, qualified label that uniquely identifies them in the
project (something like `fbcode//antlir/antlir2/features/rpm/tests:simple`).

Within the same [buildfile](#buildfile), targets can be referenced using just
their name prefixed with a `:` (something like `":simple"`).

### Artifact

An `artifact` is a file or directory produced by a [rule](#rule).

A [target](#target) may have zero or more output artifacts (which can be
produced with `buck2 build`), but generally user-facing targets (think
`rust_binary`, `image.layer`, etc) have exactly one.

### Macro

A macro is a function called from a [buildfile](#buildfile) (or transitively by
a function called from a buildfile).

#### What separates a macro from a function?

Technically, macros just are functions. In our lexicon though, macros will
always (transitively) expand to one or more [targets](#target) via one or more
[rule](#rule) function calls, while functions contain pure logic.

### Rule

Not to be confused with [macros](#macro), rules are a
[first class concept in buck2](https://buck2.build/docs/rule_authors/writing_rules/)
that let us write [starlark](#starlark) code that runs at
[analysis time](#analysis-time) to determine how buck2 should build our
[targets](#target).

A rule can produce zero or more [artifacts](#artifact) by running commands
(either locally or via
[Remote Execution](https://buck2.build/docs/users/remote_execution/)), or
copying/symlinking other artifacts.

Rules must return [providers](#provider), which is how both buck2 and other rule
implementations can interact with targets.

### Provider

A Provider is some [starlark](#starlark) struct that can be attached to a
[target](#target).

Providers can record any metadata about a target that the [rule](#rule)
implementation had access to. Some examples from antlir2 where data is stored in
providers and used by our rule implementations at
[analysis time](#analysis-time)

- `flavor` of an `image.layer`
- `parent_layer` of an `image.layer`
- `rpms` available in a `repo`

## How buck2 works

:::caution

This is an ELI5 level description

A very over-simplified description of `buck2 build` follows here, please do not
take it as an accurate description of what's really going on. Instead, read it
as a general framework for how to understand the capabilities of code at
different times, and how things are generally organized.

:::

### Parse Time

This is what we call the process by which buck2 parses [buildfiles](#buildfile),
and executes the [macro](#macro) calls within.

This phase imposes some strict limitations on what code can do, but also is the
only time certain things can be done.

#### What can we do?

- call other macros
- define new [targets](#target)
- add `dep`s to new targets (by string [label](#target-label))

#### What can't we do?

- inspect the target graph
- read [providers](#provider) of dependencies

### Analysis Time

After buck2 [parses](#parse-time) all the [buildfiles](#buildfile), it needs to
determine what [artifacts](#artifact) to build and how.

This process is called _analysis_. It is in this phase where our [rule](#rule)
implementations get access to all of the [providers](#provider) of their
dependencies, and can decide what actions to execute to produce some
`artifacts`.

Analysis is where all the interesting antlir2 features happen, which other parts
of this doc site will explain in more detail.

### Execution Time

After [analysis](#analysis-time), buck2 will execute all the actions determined
by the [rules](#rule) and materialize the necessary [artifacts](#artifact).
