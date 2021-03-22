mod inline { use anyhow :: Result ; fn function_in_inline_mod () -> Result < () > { Ok (()) } mod nested_inline { fn function_in_nested_inline () -> Result < () > { Ok (()) } } } mod ext ;
