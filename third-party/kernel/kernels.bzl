def _get(kernel_or_alias, arch = "x86_64"):
    # buildifier: disable=print
    print("TODO: kernels.get({}, {})".format(kernel_or_alias, arch))

def _selection(name, query, oncall):
    # buildifier: disable=print
    print("TODO: kernels.selection({}, {}, {})".format(name, query, oncall))

kernels = struct(
    get = _get,
    select = struct(
        selection = _selection,
    ),
    all_kernels = [],
)
