## Third-party Libraries

Here are defined the set of needed libraries that come from external sources, hence the name
`third-party`.  They are organized by the ecosystem they come from.

### `python`

This contains python3 libraries that are provided by the default `pypi` repositories.  Only
`python3` is supported.  Adding or updating libraries here is currently a manual process.

### `rust`

Rust crates used to be referenced by a generated `rust/BUCK` produced by reindeer.  That
generator has been deleted, so `//third-party/rust:...` targets do not currently resolve.  Only
handwritten targets (such as `rust/bindgen`) live here today.

### `cxx`, `kernel`, `source`

Handwritten targets for system C++ toolchain bits, kernel artifacts, and sources built from
upstream tarballs.
