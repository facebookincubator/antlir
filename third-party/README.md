## Third-party Libraries

Here are defined the set of needed libraries that come from external sources, hence the name
`third-party`.  They are organized by `platform` where platform currently consists of 3 things:

### `bazel-skylib`
This contains a set of `starlark` macros used by this tool.  This is a `git submodule` that resolves
to a specific hash.  It's best to leave this alone.

### `fedora33`

This contains binaries/libraries provided as part of `fedora33` that are packaged as `rpm` files.
Adding or updating libraries here is currently a manual process.  Talk to @zeroxoneb if you need to
add or change this list.

### `python`

This contains python3 libraries that are provided the default `pypi` repositories.  Only `python3`
is supported.  Adding or updating libraries here is currently a manual process.  Talk to @zeroxoneb
if you need to modify anything here.

# hello