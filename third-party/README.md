## Third-party Libraries

Here are defined the set of needed libraries that come from external sources, hence the name
`third-party`.  They are organized by the ecosystem they come from.

### `python`

This contains python3 libraries that are provided by the default `pypi` repositories.  Only
`python3` is supported.  Adding or updating libraries here is currently a manual process.

### `rust`

Rust crates are referenced by a generated `rust/BUCK` produced by reindeer from
the committed `rust/Cargo.toml` + `rust/Cargo.lock` + `rust/fixups/` inputs.
The generated file is committed and downloads checksum-pinned crate archives
at build time. CI uses the committed targets directly.
To regenerate it locally, install reindeer
(`cargo install --locked --git https://github.com/facebookincubator/reindeer reindeer`)
and run `reindeer --third-party-dir third-party/rust --cargo-options=--locked buckify`.
To add or update a crate, edit `rust/Cargo.toml`,
regenerate the lockfile (`cargo generate-lockfile`, or
`reindeer generate-lockfile` for a deterministic minimal resolve), re-run
buckify, and commit the updated manifest inputs and generated `rust/BUCK`.

Handwritten targets (such as `rust/bindgen`) also live here.

### `cxx`, `kernel`, `source`

Handwritten targets for system C++ toolchain bits, kernel artifacts, and sources built from
upstream tarballs.
