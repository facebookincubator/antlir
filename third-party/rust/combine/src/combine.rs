use quote::ToTokens;
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use syn::token::Brace;
use syn::visit_mut::{self, VisitMut};
use syn::ItemMod;

use anyhow::Result;

struct ModVisitor;

impl VisitMut for ModVisitor {
    fn visit_item_mod_mut(&mut self, node: &mut ItemMod) {
        println!("Module with name={}", node.ident);

        if node.content.is_none() {
            // TODO: this only handles one level of nesting, but I think that is
            // good enough
            let mut include =
                File::open("/home/vmagro/antlir-git/third-party/rust/combine/src/ext.rs").unwrap();
            let mut content = String::new();
            include.read_to_string(&mut content).unwrap();
            let tree = syn::parse_file(&mut content).unwrap();
            node.content = Some((Brace::default(), tree.items));
            node.semi = None;
            println!("{:#?}", node);
            return;
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

    ModVisitor.visit_file_mut(&mut syntax);
    let ts = syntax.to_token_stream();

    let mut out = File::create(out_file)?;
    write!(out, "{}\n", ts)?;
    println!("{}", ts);

    Ok(())
}
