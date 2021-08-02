# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_tagger.bzl", "image_source_as_target_tagged_shape", "new_target_tagger", "target_tagged_image_source_shape", "target_tagger_to_feature")

rpm_action_item_t = shape.shape(
    action = shape.enum("install", "remove_if_exists"),
    source = shape.field(target_tagged_image_source_shape, optional = True),
    name = shape.field(str, optional = True),
    version_set = shape.field(shape.path(), optional = True),
    flavor_and_version_set = shape.field(shape.list(shape.tuple(str, str)), optional = True),
)

def _rpm_name_or_source(name_source):
    # Normal RPM names cannot have a colon, whereas target paths
    # ALWAYS have a colon. `image.source` is a struct.
    if not types.is_string(name_source) or ":" in name_source:
        return "source"
    else:
        return "name"

# It'd be a bit expensive to do any kind of validation of RPM
# names at this point, since we'd need the repo snapshot to decide
# whether the names are valid, and whether they contain a
# version or release number.  That'll happen later in the build.
def _build_rpm_feature(rpmlist, action, needs_version_set):
    target_tagger = new_target_tagger()
    res_rpms = []
    for path in rpmlist:
        source = None
        name = None
        version_set = None
        if _rpm_name_or_source(path) == "source":
            source = image_source_as_target_tagged_shape(target_tagger, path)
        else:
            name = path
            if needs_version_set:
                version_set = name
        rpm_action_item = shape.new(
            rpm_action_item_t,
            action = action,
            source = source,
            name = name,
            version_set = version_set,
        )
        res_rpms.append(rpm_action_item)
    return target_tagger_to_feature(
        target_tagger = target_tagger,
        items = struct(rpms = res_rpms),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image_actions:rpms"],
    )

def image_rpms_install(rpmlist):
    """
`image.rpms_install(["foo"])` installs `foo.rpm`,
`image.rpms_install(["//target:bar"])` builds `bar` target and installs
resulting RPM.

The argument to both functions is a list of RPM package names to install,
without version or release numbers. Dependencies are installed as needed.
Order is not significant.

As shown in the above example, RPMs may also be installed that are the
outputs of another buck rule by providing a target path or an `image.source`
(docs in`image_source.bzl`), or by directly providing a target path.

If RPMs are specified by name, as in the first example above, the default
behavior is to install the latest available version of the RPMs. Particular
versions of RPMs can be pinned by specifying `image.opts` with
`rpm_version_set_overrides` argument. This argument must be the list of
structures defined by `image.rpm.nevra()`:

```
image.layer(
    name = "my_layer",
    features = [
        image.rpms_install([
            "foo",
        ]),
    ],
    flavor_config_override = image.opts(
        rpm_version_set_overrides = [
            image.rpm.nevra(
                name = "foo",
                epoch = "0",
                version = "1",
                release = "el7",
                arch = "x86_64"
            ),
        ],
    ),
)
```

In this example `foo-1-el7.x86_64` will be installed into the layer `my_layer`
even if a newer version is available.

If the argument `rpmlist` lists both RPM name and buck rule targets, RPMs
specified by buck rule targets are installed before RPMs specified by names.
Hence, if an RPM defined by name requires a newer version of an RPM defined by
buck rule target, the RPM will be upgraded and the whole operation may succeed.
Thus, the explicit specification of RPM version by buck rule does not guarantee
that this particular version is present in resulting image.

Another important caveat about RPMs specified by buck rule targets is that
downgrade is allowable: if the parent layer has RPM `foobar-v2` installed, and
then `foobar-v1` is specified by a buck rule, the result of RPM installation
will be `foobar-v2` downgraded to `foobar-v1`.

`image.rpms_install()` provides only limited support for RPM post-install
scripts. Those scripts are executed in a virtual environment without runtime
mounts like `/proc`. As an example, the script may invoke a binary requiring
`/proc/self/exe` or a shared library from a directory not available in the
image. Then the binary fails, and the final result of the operation would differ
from the RPM installation on the host where the binary succeeds. The issue may
be aggravated by the lack of error handling in the script making the RPM install
operation successful even if the binary fails.
    """

    return _build_rpm_feature(rpmlist, "install", needs_version_set = True)

def image_rpms_remove_if_exists(rpmlist):
    """
`image.rpms_remove_if_exists(["baz"])` removes `baz.rpm` if exists.

Note that removals may only be applied against the parent layer -- if your
current layer includes features both removing and installing the same
package, this will cause a build failure.
    """
    return _build_rpm_feature(
        rpmlist,
        "remove_if_exists",
        needs_version_set = False,
    )
