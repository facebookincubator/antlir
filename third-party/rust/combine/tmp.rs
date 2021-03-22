mod inline {
    fn function_in_inline_mod() {}
    mod nested_inline {
        fn function_in_nested_inline() -> Result<()> {
            Ok(())
        }
    }
}
mod ext {
    fn function_in_external_mod() {}
}
#[cfg(test)]
mod tests_inline {}
#[cfg(test)]
mod tests_ext {
    fn function_in_external_mod() {}
}
