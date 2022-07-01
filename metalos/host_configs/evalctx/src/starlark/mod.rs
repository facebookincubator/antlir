use starlark::environment::Globals;
use starlark::environment::GlobalsBuilder;

pub mod generator;
pub mod loader;
pub mod template;

pub fn metalos(builder: &mut GlobalsBuilder) {
    builder.struct_("metalos", |builder: &mut GlobalsBuilder| {
        generator::module(builder);
        template::module(builder);
    });
}

pub fn globals() -> Globals {
    GlobalsBuilder::extended().with(metalos).build()
}

#[cfg(test)]
mod tests {
    use super::metalos;
    use starlark::assert::Assert;
    #[test]
    fn starlark_module_exposed() {
        let mut a = Assert::new();
        a.globals_add(metalos);
        a.pass("metalos.template(\"\")");
    }
}
