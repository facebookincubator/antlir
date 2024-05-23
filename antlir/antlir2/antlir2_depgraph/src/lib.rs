/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fmt::Display;
use std::path::Path;
use std::time::Duration;

use antlir2_depgraph_if::item;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_depgraph_if::RequiresProvides as _;
use antlir2_depgraph_if::Validator;
use antlir2_facts::fact::dir_entry::DirEntry;
use antlir2_facts::fact::Fact as _;
use antlir2_facts::RoDatabase;
use antlir2_facts::RwDatabase;
use antlir2_features::Feature;
use fxhash::FxHashMap;
use rusqlite::backup::Backup;
use rusqlite::DatabaseName;
use rusqlite::OptionalExtension as _;
use serde::Deserialize;
use serde::Serialize;
use tracing::warn;

mod fact_interop;
use fact_interop::FactExt as _;
use fact_interop::ItemKeyExt as _;
mod plugin;
mod resolve;
mod toposort;
use plugin::FeatureWrapper;

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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Cycle(Vec<Feature>);

impl Display for Cycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for feature in &self.0 {
            writeln!(f, "  {feature:?}")?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cycle in dependency graph:\n{0}")]
    Cycle(Cycle),
    #[error("{item:?} is provided by multiple features: {features:#?}")]
    Conflict {
        item: Item,
        features: BTreeSet<Feature>,
    },
    #[error("{key:?} is required by {required_by:#?} but was never provided")]
    MissingItem { key: ItemKey, required_by: Feature },
    #[error(
        "{item:?} does not satisfy the validation rules: {validator:?} as required by {required_by:#?}"
    )]
    Unsatisfied {
        item: Item,
        validator: Validator,
        required_by: Feature,
    },
    #[error("failure determining 'provides': {0}")]
    Provides(String),
    #[error("failure determining 'requires': {0}")]
    Requires(String),
    #[error("failed to deserialize feature data: {0}")]
    DeserializeFeature(serde_json::Error),
    #[error("failed to (de)serialize graph data: {0}")]
    GraphSerde(serde_json::Error),
    #[error(transparent)]
    Plugin(#[from] antlir2_features::Error),
    #[error("facts db error: {0}")]
    Facts(#[from] antlir2_facts::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct GraphBuilder {
    memdb: RwDatabase,
}

impl GraphBuilder {
    pub fn new(parent: Option<Graph>) -> Result<Self> {
        // use an in-memory database for the temporary graph changes
        let mut memdb = RwDatabase::create(":memory:")?;
        if let Some(parent) = parent {
            Backup::new(parent.db.as_ref(), memdb.as_mut())?.run_to_completion(
                128,
                Duration::from_millis(0),
                None,
            )?;
        }
        let conn = memdb.as_mut();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS feature (id INTEGER PRIMARY KEY, value TEXT NOT NULL, pending BOOLEAN NOT NULL)",
            [],
        )?;
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
        )?;
        conn.execute(
            r#"CREATE TABLE IF NOT EXISTS provides (
                feature INTEGER NOT NULL,
                item INTEGER NOT NULL,
                FOREIGN KEY(feature) REFERENCES feature(id),
                FOREIGN KEY(item) REFERENCES item ON DELETE CASCADE
            )"#,
            [],
        )?;
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
        )?;

        let tx = conn.transaction()?;
        // mark parent features as complete
        tx.execute("UPDATE feature SET pending = FALSE", [])?;
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
        )?;

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
            )?;
        }
        tx.commit()?;
        Ok(Self { memdb })
    }

    pub fn add_feature(&mut self, feature: Feature) -> Result<&mut Self> {
        let tx = self.memdb.as_mut().transaction()?;
        tx.execute(
            "INSERT INTO feature (value, pending) VALUES (?, ?)",
            (
                serde_json::to_string(&feature).map_err(Error::GraphSerde)?,
                true,
            ),
        )?;
        let feature_id = tx.last_insert_rowid();
        let provides = FeatureWrapper(&feature)
            .provides()
            .map_err(Error::Provides)?;
        for item in provides {
            let key = item.key();
            tx.execute(
                "INSERT INTO item (key, value, fact_kind, fact_key) VALUES (?, ?, ?, ?)",
                (
                    serde_json::to_string(&key).map_err(Error::GraphSerde)?,
                    serde_json::to_string(&item).map_err(Error::GraphSerde)?,
                    key.fact_kind(),
                    key.to_fact_key().as_ref(),
                ),
            )?;
            let item_id = tx.last_insert_rowid();
            tx.execute(
                "INSERT OR IGNORE INTO provides (feature, item) VALUES (?, ?)",
                (feature_id, item_id),
            )?;
        }
        let requires = FeatureWrapper(&feature)
            .requires()
            .map_err(Error::Requires)?;
        for req in requires {
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
            )?;
        }

        tx.commit()?;
        Ok(self)
    }

    /// Fixup graph edges so that any requirements pointing to a path get
    /// duplicated with an edge that points to the final, canonical path (after
    /// resolving any symlinks)
    fn fixup_symlinks(&mut self) -> Result<()> {
        let tx = self.memdb.as_mut().transaction()?;
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
            .query_and_then((DirEntry::kind(),), |row| {
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
                                DirEntry::kind(),
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
        if let Some((key, feature)) = self
            .memdb
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
            Err(Error::MissingItem {
                key,
                required_by: feature,
            })
        } else {
            Ok(())
        }
    }

    fn verify_no_invalid_deps(&self) -> Result<()> {
        for row in self
            .memdb
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
            .memdb
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
        toposort::toposort(self.memdb.as_ref())?;

        Ok(Graph {
            db: self.memdb.to_readonly()?,
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

    pub fn builder(parent: Option<Self>) -> Result<GraphBuilder> {
        GraphBuilder::new(parent)
    }

    /// Iterate over features in topographical order (dependencies sorted before the
    /// features that require them).
    pub fn pending_features(&self) -> Result<impl Iterator<Item = Feature>> {
        let features = toposort::toposort(self.db.as_ref())?;
        Ok(features.into_iter())
    }

    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        self.db
            .as_ref()
            .backup(DatabaseName::Main, path, None)
            .map_err(Error::from)
    }
}
