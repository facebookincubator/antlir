"""
The core `image.layer` abstraction deliberately prevents execution of
arbitrary commands as part of the image build.  There are many reasons:

  - Arbitrary commands, even with no network, can easily be
    non-deterministic (e.g.  something that queries time or an RNG, or
    something that depends on the inherent entropy of the process execution
    model).  Eventually, it would be nice to integrate something like
    DetTrace, but this is out of scope for now.

  - Arbitrary commands typically use shell syntax, which is both fragile, and
    is not adequately covered by `.bzl` linters. Adding more powerful linters
    is possible (e.g. ShellCheck for shell fields), but does not make shell
    scripts as obvious to the reader as intention-oriented `.bzl` programs.

  - We value `image.feature`s, which permit order-independent composition of
    independent parts of the filesystem.  For this declarative style of
    programming to work, the compiler needs to know the exact side effects
    of evaluating a feature.

  - When executing an arbitrary command, the modified filesystem can
    arbitrarily depend on the pre-existing filesystem.  So, in order to be
    deterministic, arbitrary commands must be explicitly ordered by the
    programmer.

On the other hand, we neither can, nor should support every possible
filesystem operation as part of `antlir/compiler` core.  This is where the
"foreign layer" abstraction comes in.

A foreign layer runs a command inside the snapshot of a parent image, and
captures the resulting filesystem as its output.  It is the `antlir`
analog of a Buck `genrule`.  To encourage determinism, the command has no
network access.  You can make other build artifacts available to your build
as follows:

image.layer(
    name = '_setup_foo',
    parent_layer = '...',
    features = [
        image.make_dirs('/', 'output'),
        image.install(':foo', '/output/_temp_foo'),
    ],
)

image_foreign_layer(
    name = '_translate_foo',
    parent_layer = ':setup',
    user = 'root',
    cmd = ['/bin/sh', '-c', 'tr a-z a-Z < /output/_temp_foo > /output/FOO'],
)

image.layer(
    name = 'foo',
    parent_layer = ':_translate_foo',
    # Clean up temporary state
    features = [image.remove_path('/output/_temp_foo')],
)

Customers should not use `image_foreign_layer` directly, both because using
arbitrary commands in builds is error-prone (per the above), and because the
goal is that image build declarations be as intent-oriented as possible.

Instead, we envision library authors creating self-contained, robust,
deterministic, intent-oriented abstractions on top of `image_foreign_layer`,
and placing them in a subdirectory of `bzl/foreign/`.  For a reasonable
example, take a look at `bzl/foreign/rpmbuild`.

The general idea should be to create a layer per logical image build step,
though the macro may also create intermediate layers that are not visible to
the end user.

Layering explicitly sequences the steps, and also avails us of Buck's
caching of build outputs, so that iterating on child layers does not cost a
re-build of the parent.  To take best advantage of caching, try to put the
steps that change most frequently later in the sequence (this parallels the
best practice for developing `Dockerfile`s).

In some cases, you are not interested in the entirety of the foreign layer,
but only in a few artifacts that were built inside of it.  The example of
`rpmbuild` works this way.  Follow that same pattern to get your files:
  - Have `image_foreign_layer` leave the desired output(s) at a known path
    in the image.
  - To use the output(s) in another image, just use regular image actions
    together with `image.source(layer=':foreign-layer', path='/out')`.
  - The moment you need to use such outputs as inputs to a regular Buck
    macro, ping `antlir` devs, and we'll provide an `image.source`
    analog that copies files out of the image via `find_built_subvol`.

## Rules of `image_foreign_layer` usage:

  - Always get a code / design review from an `antlir` maintainer.
  - Do not use in `TARGETS` / `BUCK` files directly.  Instead, define a
    `.bzl` macro named `image_<intended_action>_layer`.
  - Place your macro in `antlir/bzl/foreign/<intended_action>`.
  - Do not change any core `antlir` code when adding a foreign layer.  If
    your foreign layer requires changes outside of `antlir/bzl/foreign`,
    discuss them with `antlir` maintainers first.
  - Tests are mandatory, see `antlir/bzl/foreign/rpmbuild` for a good
    example.
  - Keep your macro deterministic.  The Buck linters and runtime try to
    catch the very shallow issues, but here are some other things to think
    about:
      - Avoid mutable globals. Buck doesn't guarantee order of evaluation
        of your macros across files, so a macro that updates order-sensitive
        mutable globals can create non-determinism that breaks target
        determinators for the entire repo.
      - If you're not sure whether some container or traversal is guaranteed
        to be deterministically ordered in Buck, sort it (or check).
      - Avoid reading clocks or timestamps from the filesystem, or local
        user / group IDs, or other things that can be different between your
        dev host, and another host.

# Deliberate limitations of the `image_foreign_layer` implementation

  - No network access. This is the gateway to non-deterministic hell.
    If you're sure your use-case is "safe", talk to `antlir` maintainers
    for how to implement it correctly.

  - We will not add `--bindmount-{ro,rw}` to the container invocation.
    Normal `image.layer_mount`s in the parent will, of course, work as
    intended, but these are not meant to let you bind-mount arbitrary host
    paths, and so ought not to lead to non-determinism. As in the example
    above, `image.install` is another good way to get data into your image.

    Details on the rationale: The only paths that are safe to bind-mount
    into a container are `buck-out` build artifacts. Previously mentioned
    `image.{install,layer_mount}` should adequately address this. Doing
    runtime mounts would be less deterministic because:
      - the tree being bind-mounted will have nondeterministic `stat` metadata.
      - `nspawn` bind-mounts leave behind in the image an implicitly created
         set of dirs and files for the mountpoint, and the `stat` metadata
         for these won't be deterministic either.
"""

load(":compile_image_features.bzl", "compile_image_features")
load(":container_opts.bzl", "container_opts_t", "normalize_container_opts")
load(":image_layer_utils.bzl", "image_layer_utils")
load(":image_utils.bzl", "image_utils")
load(":shape.bzl", "shape")
load(":structs.bzl", "structs")
load(":target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

foreign_layer_t = shape.shape(
    # IMPORTANT: Be very cautious about adding keys here, specifically
    # rejecting any options that might compromise determinism / hermeticity.
    # Foreign layers effectively run arbitrary code, so we should never
    # allow access to the network, nor read-write access to files outside of
    # the layer.  If you need something from the foreign layer, build it,
    # then reach into it with `image.source`.
    cmd = shape.list(str),
    user = str,
    container_opts = container_opts_t,
)

def image_foreign_layer(
        name,
        # Allows looking up this specific kind of foreign layer rule in the
        # Buck target graph.
        rule_type,
        # The command to execute inside the layer. See the docblock for
        # details on the constraints. PLEASE BE DETERMINISTIC HERE.
        cmd,
        # Run `cmd` as this user inside the image.
        user = "nobody",
        # The name of another `image_layer` target, on top of which the
        # current layer will install its features.
        parent_layer = None,
        # A struct containing fields accepted by `_build_opts` from
        # `compile_image_features.bzl`.
        build_opts = None,
        # An `image.opts` containing keys from `container_opts_t`.
        # If you want to install packages, you will usually want to
        # set `shadow_proxied_binaries`.
        container_opts = None,
        # Future: Should foreign layers also default to user-internal as we
        # plan to do for `image.layer`?
        antlir_rule = "user-facing",
        # See the `_image_layer_impl` signature (in `image_layer_utils.bzl`)
        # for all other supported kwargs.
        **image_layer_kwargs):
    # This is not strictly needed since `image_layer_impl` lacks this kwarg.
    if "features" in image_layer_kwargs:
        fail("\"features\" are not supported in image_foreign_layer")

    target_tagger = new_target_tagger()
    image_layer_utils.image_layer_impl(
        _rule_type = "image_foreign_layer_" + rule_type,
        _layer_name = name,
        # Build a new layer. It may be empty.
        _make_subvol_cmd = compile_image_features(
            current_target = image_utils.current_target(name),
            parent_layer = parent_layer,
            features = [target_tagger_to_feature(
                target_tagger,
                struct(foreign_layer = [
                    # TODO: use the `shape.to_dict()` helper from Arnav's diff.
                    structs.to_dict(shape.new(
                        foreign_layer_t,
                        cmd = cmd,
                        user = user,
                        container_opts = normalize_container_opts(
                            container_opts,
                        ),
                    )._data),
                ]),
                extra_deps = ["//antlir/bzl:image_foreign_layer"],
            )],
            build_opts = build_opts,
        ),
        antlir_rule = antlir_rule,
        **image_layer_kwargs
    )
