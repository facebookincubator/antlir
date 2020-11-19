load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:target_tagger.bzl", "image_source_as_target_tagged_dict", "new_target_tagger", "target_tagger_to_feature")

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
        dct = {"action": action, _rpm_name_or_source(path): path}

        if dct.get("source") != None:
            dct["source"] = image_source_as_target_tagged_dict(
                target_tagger,
                dct["source"],
            )
        else:
            dct["source"] = None  # Ensure this key is populated
            if needs_version_set:
                # This gets converted to a version set target path in
                # `normalize_features`.
                dct["version_set"] = dct["name"]

        res_rpms.append(dct)
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
