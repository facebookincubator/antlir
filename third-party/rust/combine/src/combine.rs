use quote::ToTokens;
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use syn::visit_mut::{self, VisitMut};
use syn::ItemMod;

use anyhow::Result;

struct ModVisitor;

impl VisitMut for ModVisitor {
    fn visit_item_mod_mut(&mut self, node: &mut ItemMod) {
        println!("Module with name={}", node.ident);

        match &node.content {
            Some(c) => (),
            None => println!("external file: {}.rs", node.ident),
        }

        // Delegate to the default impl to visit any nested modules.
        visit_mut::visit_item_mod_mut(self, node);
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let src_root = Path::new(&args[1]);
    let out_file = Path::new(&args[2]);

    let mut src_file = File::open(src_root)?;
    let mut src = String::new();
    src_file.read_to_string(&mut src)?;

    let mut syntax = syn::parse_file(&src)?;
    let ts = syntax.to_token_stream();
    println!("{}", ts);

    ModVisitor.visit_file_mut(&mut syntax);

    let mut out = File::create(out_file)?;
    write!(out, "{}\n", ts)?;

    Ok(())
}
