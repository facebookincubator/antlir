use anyhow::{Context, Result};
use quote::ToTokens;
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use syn::token::Brace;
use syn::visit_mut::{self, VisitMut};
use syn::ItemMod;

struct ModVisitor {
    root_dir: PathBuf,
}

impl VisitMut for ModVisitor {
    fn visit_item_mod_mut(&mut self, node: &mut ItemMod) {
        println!("Module with name={}", node.ident);

        if node.content.is_none() {
            // TODO: this only handles one level of nesting, but I think that is
            // good enough
            let mut include = File::open(self.root_dir.join(format!("{}.rs", node.ident)))
                .expect("could not open module file");
            let mut content = String::new();
            include.read_to_string(&mut content).unwrap();
            let tree = syn::parse_file(&mut content).unwrap();
            node.content = Some((Brace::default(), tree.items));
            node.semi = None;
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

    let mut src_file =
        File::open(src_root).with_context(|| format!("could open root src file {:?}", src_root))?;
    let mut src = String::new();
    src_file.read_to_string(&mut src)?;

    let mut syntax = syn::parse_file(&src)?;

    let mut visitor = ModVisitor {
        root_dir: src_root.parent().unwrap().to_path_buf(),
    };

    visitor.visit_file_mut(&mut syntax);
    let ts = syntax.to_token_stream();

    let mut out = File::create(out_file)?;
    write!(out, "{}\n", ts.to_string())?;

    Ok(())
}
