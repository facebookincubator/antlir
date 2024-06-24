/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_facts::fact::dir_entry::DirEntry;
use rusqlite::Connection;
use rusqlite::OptionalExtension as _;
use tracing::error;
#[cfg(test)]
use tracing::trace;

use crate::fact_interop::FactExt;
use crate::Error;
use crate::ItemKey;
use crate::Result;

fn get_path_item(conn: &Connection, path: &Path) -> Result<Option<PathItem>> {
    let key = ItemKey::Path(path.to_owned());
    let json = conn
        .query_row(
            "SELECT value FROM item WHERE key=?",
            [serde_json::to_string(&key)
                .map_err(Error::GraphSerde)?
                .as_str()],
            |row| row.get::<_, String>("value"),
        )
        .optional()?;
    match json {
        Some(json) => serde_json::from_str::<Item>(&json)
            .map_err(Error::GraphSerde)
            .map(|item| match item {
                Item::Path(p) => Some(p),
                _ => None,
            }),
        None => Ok(None),
    }
}

fn get_fact(db: &Connection, path: &Path) -> Result<Option<DirEntry>> {
    antlir2_facts::get_with_connection(db, DirEntry::key(path)).map_err(Error::Facts)
}

#[cfg_attr(test, tracing::instrument(skip(db), ret(Debug)))]
pub(crate) fn resolve(path: &Path, db: &Connection) -> Result<Option<PathBuf>> {
    let mut canonical = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => {
                canonical.clear();
                canonical.push("/");
            }
            Component::CurDir => {}
            Component::ParentDir => {
                canonical.pop();
            }
            Component::Normal(c) => canonical.push(c),
            Component::Prefix(_) => unreachable!("only supports Unix"),
        }
        // this is super noisy but valuable during tests
        #[cfg(test)]
        trace!("resolving item at {canonical:?}");
        let item = match get_path_item(db, &canonical)? {
            Some(p) => Some(p),
            None => get_fact(db, &canonical)?.map(|fact| fact.to_item()),
        };
        match &item {
            Some(p) => match p {
                PathItem::Symlink { link: _, target } => {
                    // this is super noisy but valuable during tests
                    #[cfg(test)]
                    trace!(
                        "'{}' is a symlink to '{}'",
                        canonical.display(),
                        target.display()
                    );
                    canonical.pop();
                    canonical.push(target);
                    // recurse now because this may resolve to another symlink
                    let item = resolve(&canonical, db)?;
                    match item {
                        None => {
                            error!("symlink target '{}' did not resolve", canonical.display());
                            return Ok(None);
                        }
                        Some(p) => {
                            #[cfg(test)]
                            trace!("resolved '{}' to '{}'", canonical.display(), p.display());
                            canonical = p;
                        }
                    }
                }
                _ => {}
            },
            None => {
                error!("no item found for path '{}'", canonical.display());
                return Ok(None);
            }
        };
    }
    Ok(Some(canonical))
}

#[cfg(test)]
mod tests {
    use antlir2_facts::fact::dir_entry::FileCommon;
    use antlir2_facts::fact::dir_entry::Symlink;
    use tracing_test::traced_test;

    use super::*;
    use crate::GraphBuilder;

    #[traced_test]
    #[test]
    fn simple_direct_resolve() {
        let mut graph = GraphBuilder::new_in_memory().expect("failed to create GraphBuilder");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/foo".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::RegularFile(
                FileCommon::new("/foo/bar".into(), 0, 0, 0o644).into(),
            ))
            .expect("failed to insert fact");
        assert_eq!(
            resolve(Path::new("/foo/bar"), graph.db.as_ref()).expect("failed to resolve /foo/bar"),
            Some("/foo/bar".into()),
        );
    }

    #[traced_test]
    #[test]
    fn chases_absolute_symlinks() {
        let mut graph = GraphBuilder::new_in_memory().expect("failed to create GraphBuilder");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/foo".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Symlink(Symlink::new(
                FileCommon::new("/foo/bar".into(), 0, 0, 0o777),
                "/baz".into(),
            )))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/baz".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::RegularFile(
                FileCommon::new("/baz/qux".into(), 0, 0, 0o644).into(),
            ))
            .expect("failed to insert fact");
        assert_eq!(
            resolve(Path::new("/foo/bar/qux"), graph.db.as_ref())
                .expect("failed to resolve /foo/bar/qux"),
            Some("/baz/qux".into()),
        );
    }

    #[traced_test]
    #[test]
    fn chases_sibling_symlinks() {
        let mut graph = GraphBuilder::new_in_memory().expect("failed to create GraphBuilder");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/foo".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Symlink(Symlink::new(
                FileCommon::new("/foo/bar".into(), 0, 0, 0o777),
                "baz".into(),
            )))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/foo/baz".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::RegularFile(
                FileCommon::new("/foo/baz/qux".into(), 0, 0, 0o644).into(),
            ))
            .expect("failed to insert fact");
        assert_eq!(
            resolve(Path::new("/foo/bar/qux"), graph.db.as_ref())
                .expect("failed to resolve /foo/bar/qux"),
            Some("/foo/baz/qux".into())
        );
    }

    #[traced_test]
    #[test]
    fn chases_parent_symlinks() {
        let mut graph = GraphBuilder::new_in_memory().expect("failed to create GraphBuilder");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/foo".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Symlink(Symlink::new(
                FileCommon::new("/foo/bar".into(), 0, 0, 0o777),
                "../baz".into(),
            )))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/baz".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::RegularFile(
                FileCommon::new("/baz/qux".into(), 0, 0, 0o644).into(),
            ))
            .expect("failed to insert fact");
        assert_eq!(
            resolve(Path::new("/foo/bar/qux"), graph.db.as_ref())
                .expect("failed to resolve /foo/bar/qux"),
            Some("/baz/qux".into())
        );
    }

    #[traced_test]
    #[test]
    fn chases_chain_of_symlinks() {
        let mut graph = GraphBuilder::new_in_memory().expect("failed to create GraphBuilder");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/foo".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Symlink(Symlink::new(
                FileCommon::new("/foo/bar".into(), 0, 0, 0o777),
                "../baz".into(),
            )))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Symlink(Symlink::new(
                FileCommon::new("/baz".into(), 0, 0, 0o777),
                "qux".into(),
            )))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/qux".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::RegularFile(
                FileCommon::new("/qux/corge".into(), 0, 0, 0o644).into(),
            ))
            .expect("failed to insert fact");
        assert_eq!(
            resolve(Path::new("/foo/bar/corge"), graph.db.as_ref())
                .expect("failed to resolve /foo/bar/corge"),
            Some("/qux/corge".into())
        );
    }

    #[traced_test]
    #[test]
    fn yet_another_convoluted_example() {
        let mut graph = GraphBuilder::new_in_memory().expect("failed to create GraphBuilder");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/foo".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/foo/bar".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Directory(
                FileCommon::new("/foo/baz".into(), 0, 0, 0o755).into(),
            ))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Symlink(Symlink::new(
                FileCommon::new("/qux".into(), 0, 0, 0o777),
                "foo/bar".into(),
            )))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::Symlink(Symlink::new(
                FileCommon::new("/foo/bar/oof".into(), 0, 0, 0o777),
                "../baz".into(),
            )))
            .expect("failed to insert fact");
        graph
            .db
            .insert(&DirEntry::RegularFile(
                FileCommon::new("/qux/oof".into(), 0, 0, 0o6444).into(),
            ))
            .expect("failed to insert fact");
        assert_eq!(
            resolve(Path::new("/qux/oof"), graph.db.as_ref()).expect("failed to resolve /qux/oof"),
            Some("/foo/baz".into())
        );
    }
}
