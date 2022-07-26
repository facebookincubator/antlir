---
id: rust
title: Rust
---

We now support Rust in the Antlir compiler!

## When to write Rust

Ideally, new functionality will be written in Rust. However, the Python codebase
has a lot of inertia, so we still expect to see lots of development in Python
for the time being.

Existing functionality may be ported into Rust and exposed to Python on a
case-by-case basis (basically, whenever anyone feels like it). See the Porting
section for a few tips.

## Best Practices

It should follow idiomatic standards (pass clippy, please Nick and Vinnie's
reviews, etc)

<FacebookOnly>

See [this
page](https://www.internalfb.com/intern/wiki/Rust-at-facebook/rust-best-practices/)
for a larger set of best practices

</FacebookOnly>


### API
Rust code should be written expecting the callsites to be Rust. Confine any
Python-isms to the Python interface boundary.

### Error Handling
Where reasonable, prefer using `thiserror::Error` instead of `anyhow::Error` to
provide structured errors from your library crates.

### Targets organization
Your rust crate can be defined anywhere in `antlir/`, do whatever seems the most
logical.
Python interfaces must be included in `//antlir/rust:native` (either written
there directly, or written in another target and brought in via `deps`)


## Porting Python to Rust

So you want to port a Python module to Rust? Great!

Just follow answer these questions and follow these steps and enjoy your
type-safe future!

But first, answer the following question:
Does this module have lots of Python dependencies that are not yet available in
Rust?
If yes, consider porting some of the "leaf" code (that doesn't have to call
other Antlir Python modules) you need first. This will make your life much
easier, as calling from Rust -> Python is fairly painful for complicated tasks.


### Python Interfaces

We use [pyo3](https://pyo3.rs/v0.16.4/) to expose Rust code as native Python
modules.

Due to some CPython limitations, we build all our Rust code into one monolithic
`.so` (`//antlir/rust:native`) so that internal type checking works. Don't worry
about it, you just have to know that all our Python interfaces must be exposed
as submodules of `antlir.rust`

Adding a new module is easy:
1. `load(//antlir/rust:defs.bzl", "antlir_rust_extension")
2. Define a target using `antlir_rust_extension`
3. Regenerate antlir/rust/modules.bzl by running `//antlir/rust:gen-modules-bzl`
4. Import your code from the expected module path (dir of TARGETS file plus `name`)

### .pyi Hints

The Rust code we write is strongly typed, but that doesn't expose annotations to
Python for Pyre to look at and do some static analysis of our Python code.

To be safe, each Rust-based module should have a corresponding `.pyi` that
defines the classes and functions that are defined in that module.
