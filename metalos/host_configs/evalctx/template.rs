/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::RwLock;

use anyhow::{bail, Context, Result};
use derive_more::Display;
use handlebars::Handlebars;
use once_cell::sync::Lazy;
use starlark::codemap::Span;
use starlark::environment::GlobalsBuilder;
use starlark::eval::{Arguments, Evaluator};
use starlark::values::{dict::DictOf, StarlarkValue, UnpackValue, Value, ValueLike};
use starlark::{starlark_module, starlark_simple_value, starlark_type};

static HANDLEBARS: Lazy<RwLock<Handlebars>> = Lazy::new(|| {
    let mut h = Handlebars::new();
    h.set_strict_mode(true);
    RwLock::new(h)
});

/// Template exposes the Handlebars templating language to Starlark. A Template
/// is created with the `metalos.template` function, and can be rendered by
/// calling the resulting object with the template parameters. Any Starlark
/// value that can be turned into JSON (primitives, lists, dicts, structs,
/// records, etc) can be used in a template. See the [Handlebars
/// guide](https://handlebarsjs.com/guide/) for details about the templating
/// language.
/// ```
/// hello = metalos.template("Hello {{who}}!")
/// hello(who="world") == "Hello world!"
/// ```
#[derive(Debug, Display)]
pub struct Template(String);
starlark_simple_value!(Template);

impl Template {
    pub fn compile<S: AsRef<str>>(source: S) -> Result<Self> {
        let mut hasher = DefaultHasher::new();
        source.as_ref().hash(&mut hasher);
        let name = format!("tmpl_{}", hasher.finish());
        let tmpl =
            handlebars::Template::compile(source.as_ref()).context("failed to compile template")?;
        HANDLEBARS
            .write()
            .expect("failed to lock handlebars registry")
            .register_template(&name, tmpl);
        Ok(Self(name))
    }
}

impl<'v> StarlarkValue<'v> for Template {
    starlark_type!("template");

    fn invoke(
        &self,
        _me: Value<'v>,
        _location: Option<Span>,
        args: Arguments<'v, '_>,
        eval: &mut Evaluator<'v, '_>,
    ) -> Result<Value<'v>> {
        if !args.pos.is_empty() || args.args.is_some() {
            bail!("template rendering only accepts kwargs");
        }
        let mut context: BTreeMap<String, serde_json::Value> = args
            .names
            .iter()
            .zip(args.named.iter())
            .map(|(name, param)| {
                Ok((
                    name.0.as_str().to_owned(),
                    serde_json::from_str(&param.to_json().with_context(|| {
                        format!(
                            "template kwarg {} does not support to_json",
                            name.0.as_str()
                        )
                    })?)
                    .unwrap(),
                ))
            })
            .collect::<Result<_>>()?;

        if let Some(kwargs) = args.kwargs {
            let mut kwargs = DictOf::<String, Value>::unpack_value(kwargs)
                .context("kwargs must be dict with string keys")?
                .to_dict()
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        k.clone(),
                        serde_json::from_str(&v.to_json().with_context(|| {
                            format!("template kwarg {} does not support to_json", k)
                        })?)
                        .unwrap(),
                    ))
                })
                .collect::<Result<_>>()?;
            context.append(&mut kwargs);
        }

        let out = HANDLEBARS
            .read()
            .expect("failed to read handlebars registry")
            .render(&self.0, &context)
            .context("failed to render")?;
        Ok(eval.heap().alloc(out))
    }
}

#[starlark_module]
pub fn module(registry: &mut GlobalsBuilder) {
    #[starlark(type("template"))]
    fn template(src: &str) -> Template {
        Template::compile(src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use starlark::assert::Assert;

    #[test]
    fn render_from_starlark() {
        let template = Template::compile("Hello {{ who }}!").unwrap();
        let mut a = Assert::new();
        a.globals_add(|gb| gb.set("template", template));
        a.eq("template(who=\"world\")", "\"Hello world!\"");
        a.eq("template(**{\"who\": \"world\"})", "\"Hello world!\"");
    }

    #[test]
    fn with_list() {
        let template = Template::compile("Hello {{#each who}}{{this}} {{/each}}").unwrap();
        let mut a = Assert::new();
        a.globals_add(|gb| gb.set("template", template));
        a.eq(
            "template(who=[\"whole\", \"world\"])",
            "\"Hello whole world \"",
        );
    }
}
