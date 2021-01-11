load("//antlir/bzl:oss_shim.bzl", "buck_genrule")

def worker_genrule(
        name,
        cmd,
        out):
    req = struct(
        tmp = "$TMP",
        cmd = cmd,
    )
    buck_genrule(
        name = name,
        cmd = "$(worker //antlir/vm/appliance:worker_tool) {}".format(req.to_json()),
        out = out,
    )
