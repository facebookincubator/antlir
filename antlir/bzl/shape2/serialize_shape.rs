/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// TODO(T139523690) this whole binary can be removed when target_tagger is dead,
// which will be shortly after buck1 is dead, after what I expect will be an
// extremely painful migration. In the meantime, the marked parts of this can be
// deleted as soon as we no longer support buck1.

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use absolute_path::AbsolutePathBuf;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use serde_json::Value;

#[derive(Parser)]
struct Args {
    input: PathBuf,
    output: PathBuf,
}

// TODO(T139523690) this can be removed when buck1 is dead, since all paths will
// be relative.
fn rewrite_locations<P: AsRef<Path>>(project_root: P, shape: Value) -> Value {
    match shape {
        Value::Object(mut map) => {
            if map.contains_key("__I_AM_TARGET__") {
                map.entry("path").and_modify(|path_val| {
                    // On buck1, this will be a full path, but let's serialize
                    // it as a relpath for consistency with buck2, and to make
                    // it cacheable
                    let path = Path::new(path_val.as_str().expect("'path' must be a string"));
                    let relpath = match path.strip_prefix(project_root.as_ref()) {
                        Ok(relpath) => relpath,
                        // it must have already been relative
                        Err(_) => path,
                    };
                    *path_val = relpath.to_str().expect("always valid utf8").into()
                });
                map.remove("__I_AM_TARGET__");
                map.into()
            } else {
                map.into_iter()
                    .map(|(k, v)| (k, rewrite_locations(project_root.as_ref(), v)))
                    .collect()
            }
        }
        Value::Array(arr) => arr
            .into_iter()
            .map(|v| rewrite_locations(project_root.as_ref(), v))
            .collect(),
        other => other,
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let infile = stdio_path::open(&args.input).context("while opening input")?;
    let outfile = stdio_path::create(&args.output).context("while opening output")?;
    let shape: Value = serde_json::from_reader(infile).context("while deserializing shape")?;

    let project_root = find_root::find_repo_root(
        &AbsolutePathBuf::new(std::env::current_exe().expect("could not get argv[0]"))
            .expect("argv[0] was not absolute"),
    )
    .context("while looking for repo root")?;

    let shape = rewrite_locations(&project_root, shape);

    serde_json::to_writer_pretty(&outfile, &shape).context("while serializing shape")?;
    writeln!(&outfile).context("while writing trailing newline")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_rewrite() {
        assert_eq!(
            json!({
                "hello": "world",
                "some": {
                    "nested": {
                        "target": {
                            "name": "foo//bar:baz",
                            "path": "this/is/relative",
                        }
                    }
                }
            }),
            rewrite_locations(
                "/but/it/was/absolute",
                json!({
                    "hello": "world",
                    "some": {
                        "nested": {
                            "target": {
                                "name": "foo//bar:baz",
                                "path": "/but/it/was/absolute/this/is/relative",
                                "__I_AM_TARGET__": true,
                            }
                        }
                    }
                })
            ),
        );
    }
}
