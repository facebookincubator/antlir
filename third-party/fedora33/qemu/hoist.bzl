load("//antlir/bzl:build_defs.bzl", "buck_genrule")

def hoist(name, out, layer, path, **buck_genrule_kwargs):
    """Creates a rule to lift an artifact out of the image it was built in."""
    buck_genrule(
        name = name,
        out = out,
        bash = '''
            binary_path=( $(exe //antlir:find-built-subvol) )
            layer_loc="$(location {layer})"
            sv_path=\\$( "${{binary_path[@]}}" "$layer_loc" )
            cp "$sv_path{path}" --no-clobber "$OUT"
        '''.format(
            layer = ":" + layer,
            path = path,
        ),
        **buck_genrule_kwargs
    )
