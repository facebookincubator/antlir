load("@bazel_skylib//lib:types.bzl", "types")

# py2 is dead but we still need to explicitly use a separate platform
# for OSS buck so we'll just force everything here to py3.
# In the future prehaps there will be a need for a few different
# OSS platform types (like `dev` vs `opt`), but we'll cross that bridge
# when we get to it.
_DEFAULT_PLATFORM = "py3"

def _invert_dict(d):
    """ In OSS Buck some of the dicts used by targets (`srcs` and `resources`
    specifically) are inverted, where internally this:

        resources = { "//target:name": "label_of_resource" }

    In OSS Buck this is:

        resources = { "label_of_resource": "//target:name" }
    """
    if d and types.is_dict(d):
        return {value: key for key, value in d.items()}
    else:
        return d

def _normalize_visibility(vis, name=None):
    """ OSS Buck has a slightly different handling of visibility.
    The default is to be not visible.
    For more info see: https://buck.build/concept/visibility.html
    """
    if vis == None:
        return ["PUBLIC"]
    else:
        return vis

def _normalize_pkg_style(style):
    """
    Internally, zip and fastzip internally behave similar to how an 
    `inplace` python binary behaves in OSS Buck.
    """
    if style and style in ('zip', 'fastzip'):
        return 'inplace'
    else:
        return 'standalone'

def _cpp_unittest(name, tags='ignored', visibility=None, **kwargs):
    cxx_test(
       name = name,
       visibility = _normalize_visibility(visibility),
       **kwargs
    )

def _python_binary(name, main_module, par_style=None, visibility=None, **kwargs):

    visibility = _normalize_visibility(visibility)
    python_library(
        name = name + "-library",
        visibility = visibility,
        **kwargs
    )

    python_binary(
        name = name,
        deps = [":" + name + "-library"],
        main_module = main_module,
        package_style = _normalize_pkg_style(par_style),
        platform = _DEFAULT_PLATFORM,
        visibility = visibility,
    )

def _python_library(name, deps=None, visibility=None, resources=None,
        srcs=None, **kwargs):

    python_library(
        name = name,
        deps = deps,
        resources = _invert_dict(resources),
        srcs = _invert_dict(srcs),
        visibility = _normalize_visibility(visibility, name),
        **kwargs
    )

def _python_unittest(cpp_deps='ignored', deps=None, par_style=None,
        tags='ignored', resources=None, **kwargs):

    python_test(
        platform=_DEFAULT_PLATFORM,
        deps = deps,
        package_style = _normalize_pkg_style(par_style),
        resources = _invert_dict(resources),
        **kwargs
    )


_RuleTargetProvider = provider(fields = [
    "name",  # The name of the rule
    "base_path",  # The base package within the repository
    "repo",  # Either the cell or None (for the root cell)
])

def _RuleTarget(repo, base_path, name):
    return _RuleTargetProvider(name = name, base_path = base_path, repo = repo)

def _parse_target(target, default_repo = None, default_base_path = None):
    if target.count(":") != 1:
        fail('rule name must contain exactly one ":": "{}"'.format(target))

    repo_and_base_path, name = target.split(":")

    # Parse relative target.
    if not repo_and_base_path:
        return _RuleTarget(default_repo, default_base_path, name)

    # Parse absolute targets.
    if repo_and_base_path.count("//") != 1:
        fail('absolute rule name must contain one "//" before ":": "{}"'.format(target))
    repo, base_path = repo_and_base_path.split("//", 1)
    repo = repo or default_repo

    return _RuleTarget(repo, base_path, name)

def _get_project_root_from_gen_dir():
    # NB: This will break if "buck-out" is set to something containing 1 or
    # more slashes (e.g.  `/my/buck/out`).  A fix would be to copy-pasta
    # `_get_buck_out_path`, but it seems like an unnecessary complication.
    return "../.."

def _to_label(repo, path, name):
    return "{}//{}:{}".format(repo, path, name)

shim = struct(
    buck_command_alias = command_alias,
    buck_genrule = genrule,
    buck_sh_binary = sh_binary,
    buck_sh_test = sh_test,
    cpp_unittest = _cpp_unittest,
    config = struct(
        get_current_repo_name = native.repository_name,
        get_project_root_from_gen_dir = _get_project_root_from_gen_dir,
    ),
    get_visibility = _normalize_visibility,
    platform_utils = None,
    python_binary = _python_binary,
    python_library = _python_library,
    python_unittest = _python_unittest,
    target_utils = struct(
        parse_target = _parse_target,
        to_label = _to_label,
    ),
    third_party = None
)
