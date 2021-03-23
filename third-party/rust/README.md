# Basic structure
This directory contains a `BUCK` file with targets for all the third-party
rust dependencies that we use in Antlir.

All explicit dependencies are listed in this single `Cargo.toml`, which is
used to generated `Cargo.lock` with `cargo update`. `Cargo.lock` is then used
to inform the `BUCK` file generator.

# Adding a new crate
1) add an entry to `Cargo.toml`
2) `cargo update`
3) `buck run //third-party/rust:buckify`

# third_party_rust_library implementation details
Rust does not have an analogue to Python's wheel format, so we have to
compile all third-party dependencies from source.
crates.io offers the ability to download a tarball of a crate's sources.
Unfortunately, Buck does not make `http_archive`s very easy to deal with as
individual files (we cannot use them as `srcs` in a `rust_library` rule).
Instead, we have `combine` that uses the `syn` crate to take all the source
files from a crate and combine them into one file, which is then used in the
`srcs` of a `rust_library`.

We choose not to use something like
[reindeer](https://github.com/facebookincubator/reindeer/) so that we do not
have to vendor a separate copy of sources internally, when they will only
ever be used on GitHub.