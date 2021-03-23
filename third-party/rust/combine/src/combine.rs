use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cargo_toml::Manifest;
use quote::ToTokens;
use syn::token::Brace;
use syn::visit_mut::VisitMut;
use syn::{ItemMod, Lit, Meta};

struct ModVisitor {
    root_dir: PathBuf,
}

impl VisitMut for ModVisitor {
    fn visit_item_mod_mut(&mut self, node: &mut ItemMod) {
        if node.content.is_none() {
            // TODO: this only handles one level of nesting, but I think that is
            // good enough
            let mut path = self.root_dir.join(format!("{}.rs", node.ident));
            if let Some(path_attr) = node
                .attrs
                .iter()
                .find(|a| a.path.get_ident().map(|i| i.to_string()) == Some("path".to_string()))
            {
                match path_attr.parse_meta().expect(&format!(
                    "path attribute '{}' not understood",
                    path_attr.tokens
                )) {
                    Meta::NameValue(nv) => match nv.lit {
                        Lit::Str(s) => {
                            path = self.root_dir.join(s.value());
                        }
                        _ => panic!("path attr value must be string"),
                    },
                    _ => panic!("path attr must be name-value"),
                }
            }

            let mut include =
                File::open(&path).expect(&format!("could not open module file '{:?}'", path));
            let mut content = String::new();
            include.read_to_string(&mut content).unwrap();
            let tree = syn::parse_file(&mut content).unwrap();
            node.content = Some((Brace::default(), tree.items));
            node.semi = None;
            return;
        }

        // Delegate to the default impl to visit any nested modules.
        // visit_mut::visit_item_mod_mut(self, node);
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    let crate_dir = Path::new(&args[1]);
    let manifest = Manifest::from_path(crate_dir.join("Cargo.toml"))
        .with_context(|| format!("failed to load manifest from {:?}", crate_dir))?;
    let out_file = Path::new(&args[2]);

    let lib = manifest
        .lib
        .context("Cargo.toml does not define a library")?;

    let crate_root = crate_dir.join(lib.path.unwrap_or("src/lib.rs".into()));

    let mut src_file = File::open(&crate_root)
        .with_context(|| format!("could open root src file {:?}", &crate_root))?;
    let mut src = String::new();
    src_file.read_to_string(&mut src)?;

    let mut syntax = syn::parse_file(&src)?;

    let mut visitor = ModVisitor {
        root_dir: crate_root.parent().unwrap().to_path_buf(),
    };

    visitor.visit_file_mut(&mut syntax);
    let ts = syntax.to_token_stream();

    let mut out = File::create(out_file)?;
    write!(out, "{}\n", ts)?;

    Ok(())
}
