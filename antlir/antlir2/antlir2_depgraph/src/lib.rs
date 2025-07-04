/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::path::Path;

use antlir2_depgraph_if::AnalyzedFeature;
use antlir2_depgraph_if::Validator;
use antlir2_depgraph_if::item;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_facts::RoDatabase;
use antlir2_facts::RwDatabase;
use antlir2_facts::fact::FactKind;
use antlir2_facts::fact::dir_entry::DirEntry;
use antlir2_features::Feature;
use fxhash::FxHashMap;
use rusqlite::OptionalExtension as _;
use serde::Deserialize;
use serde::Serialize;
use tracing::warn;

mod fact_interop;
use fact_interop::FactExt as _;
use fact_interop::ItemKeyExt as _;
mod error;
mod resolve;
mod toposort;
use error::ContextExt;
pub use error::Cycle;
pub use error::Error;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Edge {
    /// This feature provides an item
    Provides,
    /// This feature requires a provided item, and requires additional
    /// validation
    Requires(Validator),
    /// Simple ordering edge that does not require any additional checks
    After,
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct GraphBuilder {
    db: RwDatabase,
}

impl GraphBuilder {
    pub fn new(mut db: RwDatabase) -> Result<Self> {
        let conn = db.as_mut();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS feature (id INTEGER PRIMARY KEY, value TEXT NOT NULL, pending BOOLEAN NOT NULL)",
            [],
        ).context("while creating feature table")?;
        conn.execute(
            r#"CREATE TABLE IF NOT EXISTS item (
                id INTEGER PRIMARY KEY,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                -- fact_kind and fact_key let us associate items to facts
                -- (temporarily until they are properly consolidated)
                fact_kind TEXT NOT NULL,
                fact_key BLOB NOT NULL
            )"#,
            [],
        )
        .context("while creating item table")?;
        conn.execute(
            r#"CREATE TABLE IF NOT EXISTS provides (
                feature INTEGER NOT NULL,
                item INTEGER NOT NULL,
                FOREIGN KEY(feature) REFERENCES feature(id),
                FOREIGN KEY(item) REFERENCES item ON DELETE CASCADE
            )"#,
            [],
        )
        .context("while creating provides table")?;
        conn.execute(
            r#"CREATE TABLE IF NOT EXISTS requires (
                feature INTEGER NOT NULL,
                item_key TEXT NOT NULL,
                fact_kind TEXT NOT NULL,
                fact_key BLOB NOT NULL,
                ordered BOOLEAN NOT NULL,
                validator TEXT NOT NULL,
                FOREIGN KEY(feature) REFERENCES feature(id)
                -- item_key is not a FOREIGN key because it may not actually exist
                -- fact_{kind|key} is not a FOREIGN key because it may not actually exist
            )"#,
            [],
        )
        .context("while creating requires table")?;

        let tx = conn.transaction().context("while starting transaction")?;
        // mark parent features as complete
        tx.execute("UPDATE feature SET pending = FALSE", [])
            .context("while marking parent's pending features as complete")?;
        // remove any parent items where the facts have been deleted rendering
        // any existing requires/provides links obsolete
        tx.execute(
            r#"
                DELETE FROM item
                WHERE id IN (
                    SELECT id FROM item
                    LEFT JOIN facts ON (
                        facts.kind=item.fact_kind
                        AND facts.key=item.fact_key
                    )
                    WHERE facts.key IS NULL
                )
            "#,
            [],
        )
        .context("while deleting orphaned items")?;

        // Some items are always available, since they are a property of the
        // operating system. Add them to the graph now so that the dependency
        // checks will be satisfied.
        for (id, item) in [
            Item::Path(item::Path::Entry(item::FsEntry {
                path: Path::new("/").into(),
                file_type: item::FileType::Directory,
                mode: 0o0755,
            })),
            Item::User(item::User {
                name: "root".into(),
            }),
            Item::Group(item::Group {
                name: "root".into(),
            }),
        ]
        .into_iter()
        .enumerate()
        {
            let key = item.key();
            // The 'item' table has no unique constraints so that we can have
            // better error messages on conflicts, but we really only want one
            // of each of these ambient items, so we can just use the index in
            // this array as the primary key
            tx.execute(
                "INSERT OR IGNORE INTO item (id, key, value, fact_kind, fact_key) VALUES (?, ?, ?, ?, ?)",
                (
                    id,
                    serde_json::to_string(&key).map_err(Error::GraphSerde)?,
                    serde_json::to_string(&item).map_err(Error::GraphSerde)?,
                    key.fact_kind(),
                    key.to_fact_key().as_ref(),
                ),
            )
        .context("while inserting ambient item")?;
        }
        tx.commit().context("while committing setup transaction")?;
        Ok(Self { db })
    }

    pub fn add_feature(&mut self, feature: AnalyzedFeature) -> Result<&mut Self> {
        let tx = self
            .db
            .as_mut()
            .transaction()
            .context("while starting feature add transaction")?;
        tx.execute(
            "INSERT INTO feature (value, pending) VALUES (?, ?)",
            (
                serde_json::to_string(feature.feature()).map_err(Error::GraphSerde)?,
                true,
            ),
        )
        .context("while inserting feature")?;
        let feature_id = tx.last_insert_rowid();
        for item in feature.provides() {
            let key = item.key();
            tx.execute(
                "INSERT INTO item (key, value, fact_kind, fact_key) VALUES (?, ?, ?, ?)",
                (
                    serde_json::to_string(&key).map_err(Error::GraphSerde)?,
                    serde_json::to_string(&item).map_err(Error::GraphSerde)?,
                    key.fact_kind(),
                    key.to_fact_key().as_ref(),
                ),
            )
            .context("while inserting feature provides item")?;
            let item_id = tx.last_insert_rowid();
            tx.execute(
                "INSERT OR IGNORE INTO provides (feature, item) VALUES (?, ?)",
                (feature_id, item_id),
            )
            .context("while inserting feature provides edge")?;
        }
        for req in feature.requires() {
            tx.execute(
                "INSERT OR IGNORE INTO requires (feature, item_key, fact_kind, fact_key, ordered, validator) VALUES (?, ?, ?, ?, ?, ?)",
                (
                    feature_id,
                    serde_json::to_string(&req.key).map_err(Error::GraphSerde)?,
                    req.key.fact_kind(),
                    req.key.to_fact_key().as_ref(),
                    req.ordered,
                    serde_json::to_string(&req.validator).map_err(Error::GraphSerde)?,
                ),
            )
            .context("while inserting feature requires edge")?;
        }

        tx.commit()
            .context("while committing feature add transaction")?;
        Ok(self)
    }

    /// Fixup graph edges so that any requirements pointing to a path get
    /// duplicated with an edge that points to the final, canonical path (after
    /// resolving any symlinks)
    fn fixup_symlinks(&mut self) -> Result<()> {
        let tx = self.db.as_mut().transaction()?;
        for row in tx
            .prepare(
                r#"
            SELECT requires.item_key, requires.validator, requires.feature, requires.ordered
            FROM requires
            INNER JOIN feature
                ON feature.id=requires.feature
            WHERE
                feature.pending=1
                AND requires.fact_kind=?1
        "#,
            )?
            .query_and_then((DirEntry::KIND,), |row| {
                let item_key: ItemKey = serde_json::from_str(
                    row.get_ref("item_key")?
                        .as_str()
                        .map_err(rusqlite::Error::from)?,
                )
                .map_err(Error::GraphSerde)?;
                let validator: String = row.get("validator")?;
                let feature: i64 = row.get("feature")?;
                let ordered: bool = row.get("ordered")?;
                Result::Ok((item_key, validator, feature, ordered))
            })?
            .collect::<Result<Vec<_>>>()?
        {
            let (item_key, validator, feature, ordered) = row;
            if let ItemKey::Path(path) = &item_key {
                if let Some(canonical) = resolve::resolve(path, &tx)? {
                    let fact_key = DirEntry::key(&canonical);
                    let canonical_item_key = ItemKey::Path(canonical.clone());

                    let bare_item: Option<Item> = match tx
                        .query_row(
                            r#"SELECT value FROM item WHERE item.key=?"#,
                            [serde_json::to_string(&item_key).map_err(Error::GraphSerde)?],
                            |row| row.get::<_, String>("value"),
                        )
                        .optional()?
                        .map(|s| serde_json::from_str(&s).map_err(Error::GraphSerde))
                        .transpose()?
                    {
                        Some(item) => Some(item),
                        None => antlir2_facts::get_with_connection(&tx, DirEntry::key(path))?
                            .map(|de: DirEntry| Item::Path(de.to_item())),
                    };

                    // if the requirement points directly to a symlink, keep
                    // a requirement for it, but also add another
                    // requirement against the target
                    if matches!(bare_item, Some(Item::Path(PathItem::Symlink { .. }))) {
                        tx.execute(
                            "INSERT INTO requires (item_key, fact_kind, fact_key, validator, feature, ordered) VALUES (?, ?, ?, ?, ?, ?)",
                            (
                                serde_json::to_string(&canonical_item_key)
                                    .map_err(Error::GraphSerde)?,
                                DirEntry::KIND,
                                fact_key.as_ref(),
                                validator,
                                feature,
                                ordered,
                            ))?;
                        // replace the existing requirement validator with a
                        // simple "exists" so that actual logic does not get
                        // attempted on the symlink
                        tx.execute(
                            "UPDATE requires SET validator=? WHERE item_key=?",
                            (
                                serde_json::to_string(&Validator::Exists)
                                    .map_err(Error::GraphSerde)?,
                                serde_json::to_string(&item_key).map_err(Error::GraphSerde)?,
                            ),
                        )?;
                    } else {
                        // otherwise just point directly to the symlink's target
                        tx.execute(
                            "UPDATE requires SET item_key=?, fact_key=? WHERE item_key=?",
                            (
                                serde_json::to_string(&canonical_item_key)
                                    .map_err(Error::GraphSerde)?,
                                fact_key.as_ref(),
                                serde_json::to_string(&item_key).map_err(Error::GraphSerde)?,
                            ),
                        )?;
                    }
                } else {
                    warn!("failed to canonicalize path '{}'", path.display());
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn verify_no_missing_deps(&self) -> Result<()> {
        // TODO: we can easily detect multiple errors, but the interface in this
        // crate is to only return one, so just limit it to one error
        let x = match self
            .db
            .as_ref()
            .prepare(
                r#"
                SELECT feature.value AS feature, requires.item_key, requires.fact_kind, requires.fact_key
                    FROM feature
                    INNER JOIN requires ON feature.id=requires.feature
                    LEFT JOIN item ON item.key=requires.item_key
                    LEFT JOIN facts ON (
                        facts.kind=requires.fact_kind
                        AND facts.key=requires.fact_key
                    )
                    WHERE
                        feature.pending=1
                        AND (
                            -- dependency can be satisfied by either fact or item
                            item.id IS NULL
                            AND facts.key IS NULL
                        )
                    LIMIT 1
                "#,
            )?
            .query_and_then([], |row| {
                let feature: Feature = serde_json::from_str(
                    row.get_ref("feature")?
                        .as_str()
                        .map_err(rusqlite::Error::from)?,
                )
                .map_err(Error::GraphSerde)?;
                let item_key: ItemKey = serde_json::from_str(
                    row.get_ref("item_key")?
                        .as_str()
                        .map_err(rusqlite::Error::from)?,
                )
                .map_err(Error::GraphSerde)?;
                Result::Ok((item_key, feature))
            })?
            .next()
            .transpose()?
            {
                Some((key, feature)) => {
                    Err(Error::MissingItem {
                        key,
                        required_by: feature,
                    })
                },
                _ => Ok(())
            };
        x
    }

    fn verify_no_invalid_deps(&self) -> Result<()> {
        for row in self
            .db
            .as_ref()
            .prepare(
                r#"
                SELECT item.value AS item, requires.validator, feature.value AS feature
                    FROM feature
                    INNER JOIN requires ON feature.id=requires.feature
                    INNER JOIN item ON requires.item_key=item.key
                    WHERE feature.pending=1
                "#,
            )?
            .query_and_then([], |row| {
                let item: Item = serde_json::from_str(
                    row.get_ref("item")?
                        .as_str()
                        .map_err(rusqlite::Error::from)?,
                )
                .map_err(Error::GraphSerde)?;
                let validator: Validator = serde_json::from_str(
                    row.get_ref("validator")?
                        .as_str()
                        .map_err(rusqlite::Error::from)?,
                )
                .map_err(Error::GraphSerde)?;
                let feature: Feature = serde_json::from_str(
                    row.get_ref("feature")?
                        .as_str()
                        .map_err(rusqlite::Error::from)?,
                )
                .map_err(Error::GraphSerde)?;
                Result::Ok((item, validator, feature))
            })?
        {
            let (item, validator, feature) = row?;
            if !validator.satisfies(&item) {
                // TODO: we can easily detect multiple errors, but the interface
                // in this crate is to only return one
                return Err(Error::Unsatisfied {
                    item,
                    validator,
                    required_by: feature,
                });
            }
        }
        Ok(())
    }

    fn verify_no_conflicts(&self) -> Result<()> {
        // TODO: this does not detect conflicts in dynamically provided items
        // (such as files from rpms), but this is a long-standing bug not a new
        // regression
        let mut conflicts: FxHashMap<ItemKey, (BTreeSet<Item>, BTreeSet<Feature>)> =
            FxHashMap::default();
        for row in self
            .db
            .as_ref()
            .prepare(
                r#"
                SELECT i.key AS item_key, i.item, feature.value AS feature
                FROM (
                    SELECT id, key, value AS item, COUNT(*) AS cnt
                    FROM item
                    GROUP BY key
                    HAVING cnt > 1
                ) i
                INNER JOIN item i2 ON i2.key=i.key
                INNER JOIN provides ON i2.id=provides.item
                INNER JOIN feature ON provides.feature=feature.id
                "#,
            )?
            .query_and_then([], |row| {
                let item_key: ItemKey = serde_json::from_str(
                    row.get_ref("item_key")?
                        .as_str()
                        .map_err(rusqlite::Error::from)?,
                )
                .map_err(Error::GraphSerde)?;
                let item: Item = serde_json::from_str(
                    row.get_ref("item")?
                        .as_str()
                        .map_err(rusqlite::Error::from)?,
                )
                .map_err(Error::GraphSerde)?;
                let feature: Feature = serde_json::from_str(
                    row.get_ref("feature")?
                        .as_str()
                        .map_err(rusqlite::Error::from)?,
                )
                .map_err(Error::GraphSerde)?;
                Result::Ok((item_key, item, feature))
            })?
        {
            let (item_key, item, feature) = row?;
            let conflict = conflicts.entry(item_key).or_default();
            conflict.0.insert(item);
            conflict.1.insert(feature);
        }
        for (items, features) in conflicts.into_values() {
            let mut items = items.into_iter();
            let item = items.next().expect("must have at least one item");
            // true if all the items are equivalent
            let only_one_version = items.next().is_none();
            if let Item::Path(PathItem::Entry(fse)) = &item {
                // Two distinct features are allowed to provide the same
                // directory as long as the PathItem::Entry's are equivalent,
                // since that covers the filesystem metadata we care about
                if fse.file_type == FileType::Directory && only_one_version {
                    continue;
                }
            }
            // features that have completely identical data are a bit of an
            // anti-pattern, but not considered a conflict since they will do
            // the exact same thing
            let feature_datas: Vec<_> = features.iter().map(|f| &f.data).collect();
            if feature_datas.iter().all(|d| d == &feature_datas[0]) {
                continue;
            }

            return Err(Error::Conflict { item, features });
        }
        Ok(())
    }

    pub fn build(mut self) -> Result<Graph> {
        self.fixup_symlinks()?;
        self.verify_no_missing_deps()?;
        self.verify_no_invalid_deps()?;
        self.verify_no_conflicts()?;
        // doing the topological sort ensures that there aren't any cycles
        toposort::toposort(self.db.as_ref())?;

        Ok(Graph {
            db: self.db.to_readonly()?,
        })
    }
}

pub struct Graph {
    db: RoDatabase,
}

impl Graph {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = RoDatabase::open(path)?;
        Ok(Self { db })
    }

    pub fn builder(db: RwDatabase) -> Result<GraphBuilder> {
        GraphBuilder::new(db)
    }

    /// Iterate over features in topographical order (dependencies sorted before the
    /// features that require them).
    pub fn pending_features(&self) -> Result<impl Iterator<Item = Feature> + use<>> {
        let features = toposort::toposort(self.db.as_ref())?;
        Ok(features.into_iter())
    }
}

#[cfg(test)]
impl GraphBuilder {
    fn new_in_memory() -> Result<Self> {
        let db = RwDatabase::create(":memory:")?;
        Self::new(db)
    }
}
