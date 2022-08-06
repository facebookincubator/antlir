/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::RwLock;

use anyhow::bail;
use derive_more::Display;
use handlebars::Handlebars;
use handlebars::RenderError;
use once_cell::sync::Lazy;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Arguments;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::starlark_type;
use starlark::values::NoSerialize;
use starlark::values::ProvidesStaticType;
use starlark::values::StarlarkValue;
use starlark::values::Value;

use crate::Error;
use crate::Result;

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
#[derive(Debug, Display, ProvidesStaticType, NoSerialize)]
pub struct Template(String);
starlark_simple_value!(Template);

impl Template {
    pub fn compile<S: AsRef<str>>(source: S) -> Result<Self> {
        let mut hasher = DefaultHasher::new();
        source.as_ref().hash(&mut hasher);
        let name = format!("tmpl_{}", hasher.finish());
        let tmpl = handlebars::Template::compile(source.as_ref())?;
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
        args: &Arguments<'v, '_>,
        eval: &mut Evaluator<'v, '_>,
    ) -> anyhow::Result<Value<'v>> {
        if args.no_positional_args(eval.heap()).is_err() {
            bail!(Error::TemplateRender(RenderError::new(
                "template rendering only accepts kwargs",
            )));
        }
        let context: BTreeMap<String, serde_json::Value> = args
            .names_map()?
            .iter()
            .map(|(name, param)| {
                Ok((
                    name.as_str().to_owned(),
                    serde_json::from_str(&param.to_json().map_err(|_| {
                        Error::TemplateRender(RenderError::new(format!(
                            "template kwarg '{}' does not support to_json",
                            name.as_str(),
                        )))
                    })?)
                    .map_err(|e| {
                        Error::TemplateRender(RenderError::new(format!(
                            "failed to convert template arg '{}' to json: {:?}",
                            name.as_str(),
                            e
                        )))
                    })?,
                ))
            })
            .collect::<Result<_>>()?;
        let out = HANDLEBARS
            .read()
            .expect("failed to read handlebars registry")
            .render(&self.0, &context)?;
        Ok(eval.heap().alloc(out))
    }
}

#[starlark_module]
pub fn module(registry: &mut GlobalsBuilder) {
    #[starlark(type = "template")]
    fn template(src: &str) -> anyhow::Result<Template> {
        Template::compile(src).map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use starlark::assert::Assert;

    use super::*;

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
