/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context as _;
use anyhow::Result;
use clap::Parser;
use maplit::hashmap;
use tera::Context;
use tera::Tera;

mod buck;
mod register_templates;

struct TemplateCfg<'a> {
    pub input: &'a str,
    pub doc_location: &'a str,
}

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    out: Option<PathBuf>,
}

fn format_ty(
    value: &tera::Value,
    _args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let mut possible_types = value
        .as_str()
        .ok_or(tera::Error::msg("expected string"))?
        .split('|')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect::<BTreeSet<&str>>();

    let allows_select = possible_types.remove("selector");
    let is_optional = possible_types.remove("None");

    let possible_types: Vec<_> = possible_types.into_iter().collect();

    let mut s = possible_types.join(" | ");
    if is_optional {
        s = format!("Optional[{s}]");
    }
    if allows_select {
        s.push_str(" (selectable)");
    }
    Ok(s.into())
}

fn main() -> Result<()> {
    let starlark_path_to_template = hashmap! {
        "fbcode//antlir/antlir2/bzl/feature:defs.bzl" => TemplateCfg {
            input: "templates/features.mdx",
            doc_location: "features.md",
        },
        #[cfg(facebook)]
        "fbcode//antlir/antlir2/features/facebook/container:defs.bzl" => TemplateCfg {
            input: "templates/fb/cmv2_api_reference.mdx",
            doc_location: "fb/cmv2-api-reference.md",
        },
    };
    let args = Args::parse();

    let mut tera = Tera::default();
    register_templates::register_templates(&mut tera).context("while registering templates")?;

    for (starlark_path, template) in starlark_path_to_template {
        let feature_defs = buck::starlark_doc(starlark_path.parse().expect("valid label"))
            .context("while getting top-level feature docs")?;

        let mut context = Context::new();
        let feature_funcs: HashMap<String, buck::Function> = feature_defs
            .elements
            .into_iter()
            .filter_map(|e| match e.item {
                buck::Item::Function(f) => Some((e.id.name, f)),
                _ => None,
            })
            .collect();

        context.insert("funcs", &feature_funcs);

        tera.register_filter("format_ty", format_ty);

        let feature_defs = tera.render(template.input, &context)?;

        if let Some(out) = &args.out {
            let outpath = out.join(template.doc_location);
            std::fs::create_dir_all(outpath.parent().expect("parent dir exists"))?;
            std::fs::write(outpath, feature_defs)?;
        } else {
            println!("{}", feature_defs);
        }
    }

    Ok(())
}
