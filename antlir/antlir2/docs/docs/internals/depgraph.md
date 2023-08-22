---
sidebar_position: 2
sidebar_label: Depgraph
---

# antlir2 depgraph

`antlir2` ships with an image-aware dependency graph that sits alongside the
`buck2` dependency graph. When put together, they enable `antlir2` to
intelligently cache image layers and even phases within a single layer.


## Build Phases

A porcelain `image.layer` target is internally represented as one or more
`image.layer`s chained together with `parent_layer`, depending on the `features`
that are being used.

Each `BuildPhase` results in a single `antlir2` build operation that produces
exactly one of these internal layers at a time. Within that build, the [antlir2
depgraph](#antlir2-depgraph) is used for ordering features.

Every `feature` type corresponds to a default `BuildPhase`, used to run
unpredictable features (where `antlir2` cannot determine what files that feature
requires or produces) in a well-defined order.

`build_phase.bzl` has a complete list of the `BuildPhase`s that `antlir2` uses,
but it generally operates in this order:
1. Install packages
1. Run arbitrary genrules
1. Removals with `feature.remove`
1. Well behaved features (everything else)
1. Build info stamping

In this page, the term "layer" generally refers to one of these internal layers
for each phase, rather than the porcelain `image.layer` target.

## buck2 target graph

The buck2 dependency graph is the top-level mechanism that causes antlir2
rebuilds.

If the input to a feature changes (for example, a source file changed that
causes a binary to be rebuilt, and that binary is installed in an image),
everything downstream of that feature must be rebuilt.

If the input to a layer changes (for example, a feature changed, or its
`parent_layer` changed), that layer and everything downstream of it must be
rebuilt.

## antlir2 depgraph

The `antlir2` depgraph is used only for correctness, not cache invalidation
(`buck2` simply does not invoke `antlir2` if nothing has changed).

"Correctness" has a few key components:
* Ordering features so they compile safely - examples:
  * Parent directories need to exist before installing files
  * User must exist before extending their group membership
* Entities satisfy certain requirements - examples:
  * User homedir must exist and be a directory
  * Symlink targets need to exist
  * Running a command requires argv[0] to exist and be executable
* Failing on any conflicts - examples:
  * Install both 'a' and 'b' to '/c'
  * Overwrite file installed by an rpm
  * Create same user twice with different settings


### Requires / Provides

Requires and Provides are the mechanisms by which `antlir2` can provide the
correctness properties described above.

Every feature must declare what entities (files, users, etc) it creates, and any
that it requires (either before or concurrently to) building.

When this information is exhaustive - as it is for well-behaved features (not
rpms or genrules) - this is enough for `antlir2` to detect conflicts, any
missing dependencies and lastly topologically sort features to be executed in
the correct order)
