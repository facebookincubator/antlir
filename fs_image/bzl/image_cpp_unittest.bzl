load(":oss_shim.bzl", "buck_genrule", "cpp_unittest", "get_visibility", "python_binary")
load(":image_unittest_helpers.bzl", helpers = "image_unittest_helpers")

def image_cpp_unittest(
        name,
        layer,
        boot = False,
        run_as_user = "nobody",
        visibility = None,
        hostname = None,
        serve_rpm_snapshots = (),
        **cpp_unittest_kwargs):
    visibility = get_visibility(visibility, name)

    wrapper_props = helpers.nspawn_wrapper_properties(
        name = name,
        layer = layer,
        test_type = "gtest",
        boot = boot,
        run_as_user = run_as_user,
        inner_test_kwargs = cpp_unittest_kwargs,
        extra_outer_kwarg_names = [],
        caller_fake_library = "//fs_image/bzl:image_cpp_unittest",
        visibility = visibility,
        hostname = hostname,
        serve_rpm_snapshots = serve_rpm_snapshots,
    )

    cpp_unittest(
        name = helpers.hidden_test_name(name),
        tags = helpers.tags_to_hide_test(),
        visibility = visibility,
        fs_image_internal_rule = True,
        **wrapper_props.inner_test_kwargs
    )

    wrapper_binary = "layer-test-wrapper-" + name
    python_binary(
        name = wrapper_binary,
        main_module = "fs_image.nspawn_in_subvol.run_test",
        deps = [wrapper_props.impl_python_library],
        # Ensures we can read resources in @mode/opt.  "xar" cannot work
        # because `root` cannot access the content of unprivileged XARs.
        par_style = "zip",
        visibility = visibility,
        fs_image_internal_rule = True,
    )

    # Here, we generate a C file, whose only job is to `execv` the Python
    # binary that executes our `cpp_unittest` in a container.  It has to be
    # a C program so that it can act as the main of the outer `cpp_unittest`
    # below (its doc hints how to make this hack unnecessary).
    #
    # Naively, we want to call `execv($(location :<wrapper_binary>),
    # argv);`.  However, the resulting C file, and thus outer `cpp_unittest`
    # would end up containing a local filesystem path, instantly breaking
    # these tests if the artifacts got pulled from Buck's distributed cache.
    #
    # So instead, the C file just contains the `basename` of the output
    # path, which should be stable. Then, we trust that the outer test
    # will be a sibling of `wrapper_binary` in `buck-out`, and compute
    #
    #     dirname(argv[0]) + "/" + basename($(location :<wrapper_binary>))
    #
    # at test runtime.  The good news: if Buck ever breaks this convention,
    # CI will tell us promptly.
    exec_wrapper_c = "layer-test-exec-wrapper-c-" + name
    buck_genrule(
        name = exec_wrapper_c,
        out = "exec_nspawn_wrapper.c",
        cmd = """
        set -e
        # Buck macro expansions are (hopefully) shell-escaped.  This
        # Bash+Python pipeline takes the basename of the `wrapper_binary`
        # output path, and converts the shell-escaping to C-escaping.
        echo -n $(location :""" + wrapper_binary + """) | python3 -c '\
import os.path, sys
print(sys.argv[1].format(
    wrapper_filename="".join(
        # Do not escape printable chars besides backslash or double-quote
        chr(c) if c not in [34, 92] and (32 <= c <= 127)
            else "\\{:03o}".format(c)
        for c in os.path.basename(sys.stdin.buffer.read())
    ),
))
        ' '\
#include <errno.h>
#include <libgen.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {{
    (void) argc;  // Many FB codebases build with -Wunused-parameter
    const char* my_dir = dirname(argv[0]);
    const char* wrapper_binary = "{wrapper_filename}";
    // The 2 extra bytes are slash & nul
    const size_t len = strlen(my_dir) + strlen(wrapper_binary) + 2;
    char *wrapper_path = calloc(len, 1);
    strncat(wrapper_path, my_dir, len - 1);
    strncat(wrapper_path, "/", len - 1);
    strncat(wrapper_path, wrapper_binary, len - 1);
    execv(wrapper_path, argv);
    return errno;
}}
' > "$OUT"
        """,
        visibility = visibility,
        fs_image_internal_rule = True,
    )

    env = wrapper_props.outer_test_kwargs.pop("env")
    env.update({
        # These dependencies must be on the user-visible "porcelain"
        # target, see the helper code for the explanation.
        "_dep_for_test_wrapper_{}".format(idx): "$(location {})".format(
            target,
        )
        for idx, target in enumerate(wrapper_props.porcelain_deps + [
            # Without this extra dependency, Buck will fetch the
            # `cpp_unittest` from cache without also fetching
            # `wrapper_binary`.  However, `exec_nspawn_wrapper.c` needs
            # `wrapper_binary` to be present in the local `buck-out`.
            ":" + wrapper_binary,
        ])
    })

    # This is a `cpp_unittest` for reasons very similar to why the wrapper
    # binary in `image_python_unittest.bzl` is a `python_unittest`.  We
    # could eliminate all of the above contortions if Buck adds support for
    # passing through a handful of arguments from its `sh_test` into the
    # JSON info that is handed to test runners, see Q18889.
    cpp_unittest(
        name = name,
        srcs = [":" + exec_wrapper_c],
        env = env,
        use_default_test_main = False,
        visibility = visibility,
        **wrapper_props.outer_test_kwargs
    )
