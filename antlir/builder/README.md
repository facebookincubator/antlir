Builder
=======

Naming things is hard... This intended as the top-level entrypoint for building
an image with Buck. Buck rules should (eventually) interact _only_ with this
binary, and it should be responsible for setting up `buck-image-out`, providing
subvolumes and feature info the compiler etc.

As a transitionary step until most of of this code is consumed via
`rust_library` targets in separate binaries, this provides a way to run any
arbitrary command (such as `//antlir:compiler` or plain-old `bash`) in an
environment that ensures `buck-image-out` is setup correctly and that build logs
go to `image_build.log` etc.
Longer term, the right way to do this is not via this wrapper binary, but via
`rust_library` targets that gradually appear in later diffs in this stack. The
reason for this ideal is that we will get much stronger compile-time safety when
more things are direct Rust, rather than this hard-to-assert boundary of
spawning a random subprocess.

This binary should gradually take over all the functions of the
`//antlir:compiler` that are not directly related to feature items (such as
creating subvolumes, receiving sendstreams etc).


Buck2
-----

This setup is much more conducive to migrating to `buck2` while still retaining
compatibility with `buck1`. This binary can translate the less expressive inputs
(read: target tagger) from Antlir `buck1` macros into the better versions we can
provide with `buck2` and that translation code can just be deleted once `buck1`
is gone.
