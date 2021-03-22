mod inline {
    fn function_in_inline_mod() {}
    mod nested_inline {
        fn function_in_nested_inline() -> Result<()> {
            Ok(())
        }
    }
}

mod ext;

#[cfg(test)]
mod tests_inline {}

#[cfg(test)]
mod tests_ext;
