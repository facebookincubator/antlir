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

### `rust`

Rust crates are exposed as a submodule, the source of which is another branch
in this repository. This allows us to avoid vendoring another copy of all
these third-party deps internally. This is a little inconvenient to deal
with, but these crates change relatively infrequently, and not vendoring the
sources in fbcode is a huge benefit to general usability.

The submodule is managed by
[reindeer](https://github.com/facebookincubator/reindeer/) which vendors the
sources and generates buck targets.
To add/remove/update a crate:
1) checkout the `rust-reindeer` branch in git
2) make the necessary change in `Cargo.toml`
3) run `reindeer vendor && reindeer buckify`
4) push the change to GitHub
5) update the commit in `rust.submodule.txt` (or checkout the new revision
before submitting a PR)
