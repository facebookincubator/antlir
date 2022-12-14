Directory for code that is in development and is good enough to be reviewed and
to land for iterative development purposes, but not yet ready for the rest of
`//antlir/...` to take dependencies on it.

Visibility for targets using Antlir's `build_defs.bzl` are locked to this
directory only, preventing this work-in-progress code from leaking into the rest
of Antlir before it's ready.
