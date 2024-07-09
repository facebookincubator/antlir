# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def _impl(ctx: AnalysisContext) -> list[Provider]:
    src = """
    pub(crate) fn register_templates(tera: &mut ::tera::Tera) -> ::tera::Result<()> {
        tera.add_raw_templates(vec![
    """
    src += ",".join([
        "(\"{}\", include_str!(\"../{}\"))".format(f.short_path, f.short_path)
        for f in ctx.attrs.templates
    ])
    src += "])}"
    src = ctx.actions.write("register_templates_src.rs", src)
    return [DefaultInfo(src)]

register_templates_src = rule(
    impl = _impl,
    attrs = {
        "templates": attrs.list(attrs.source()),
    },
)
