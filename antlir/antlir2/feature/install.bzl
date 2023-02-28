# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:stat.bzl", "stat")
load(":feature_info.bzl", "InlineFeatureInfo")

def install(
        *,
        src: str.type,
        dst: str.type,
        mode: [int.type, str.type, None] = None,
        user: str.type = "root",
        group: str.type = "root",
        separate_debug_symbols = True) -> InlineFeatureInfo.type:
    # the default mode is determined later, after we know if the thing being
    # installed is a binary or not
    mode = stat.mode(mode) if mode else None

    # This may be a dep or a direct source file, if it has a ':' in it, put it
    # into the 'deps' dict, otherwise it goes in 'sources'
    # Technically we could get by with only setting this in `sources`, but then
    # we'd lose the ability to automatically determine the mode for executables
    src_dict = {"src": src}

    return InlineFeatureInfo(
        feature_type = "install",
        deps = src_dict if ":" in src else None,
        sources = src_dict if ":" not in src else None,
        kwargs = {
            "dst": dst,
            "group": group,
            "mode": mode,
            "separate_debug_symbols": separate_debug_symbols,
            "user": user,
        },
    )

def install_to_json(
        dst: str.type,
        group: str.type,
        mode: [int.type, None],
        user: str.type,
        separate_debug_symbols: bool.type,
        sources: {str.type: "artifact"} = {},
        deps: {str.type: "dependency"} = {}) -> {str.type: ""}:
    if "src" in deps:
        src = ensure_single_output(deps["src"])

        # Unfortunately we can only determine `mode` automatically if the dep is
        # an executable, since a plain source might be a directory
        if not mode and RunInfo in deps["src"]:
            # There is no need for the old buck1 `install_buck_runnable` stuff
            # in buck2, since we put a dep on the binary directly onto the layer
            # itself, which forces a rebuild when appropriate.
            mode = 0o555
        if not mode:
            # We can't tell if a source is a file or directory, so we need to
            # force the user to specify it
            # https://fb.workplace.com/groups/buck2users/posts/3346711265585231
            fail("unable to automatically determine 'mode'\nplease specify it like 'mode=\"a+rx\"'")
    elif "src" in sources:
        src = sources["src"]
    else:
        fail("source was missing from both 'deps' and 'sources', this should be impossible")
    return {
        "dst": dst,
        "group": group,
        "mode": mode,
        "separate_debug_symbols": separate_debug_symbols,
        "src": src,
        "user": user,
    }
