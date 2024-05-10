/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fmt::Display;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use antlir2_features::Feature;
use buck_label::Label;
use itertools::Itertools;
use petgraph::graph::DefaultIx;
use petgraph::graph::NodeIndex;
use petgraph::stable_graph::StableGraph;
use petgraph::visit::Dfs;
use petgraph::Directed;
use petgraph::Direction;
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use serde::Deserialize;
use serde::Serialize;

pub mod item;
use item::Item;
use item::ItemKey;
pub mod requires_provides;
use requires_provides::RequiresProvides as _;
use requires_provides::Validator;
mod node;
use node::GraphExt;
pub use node::Node;

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

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
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
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct GraphBuilder {
    g: StableGraph<Node, Edge, Directed, DefaultIx>,
    root: node::RootNodeIndex,
    pending_features: Vec<node::PendingFeatureNodeIndex>,
    items: FxHashMap<ItemKey, node::ItemNodeIndex>,
    label: Label,
}

impl GraphBuilder {
    pub fn new(label: Label, parent: Option<Graph>) -> Self {
        let mut g = StableGraph::new();
        let mut items = FxHashMap::default();

        // Some items are always available, since they are a property of the
        // operating system. Add them to the graph now so that the dependency
        // checks will be satisfied.
        for item in [
            Item::User(item::User {
                name: "root".into(),
            }),
            Item::Group(item::Group {
                name: "root".into(),
            }),
            Item::Path(item::Path::Entry(item::FsEntry {
                path: Path::new("/").into(),
                file_type: item::FileType::Directory,
                mode: 0o0755,
            })),
        ] {
            let key = item.key();
            let nx = g.add_node_typed(item);
            items.insert(key, nx);
        }
        let root = g.add_node_typed(());

        let mut s = Self {
            g,
            root,
            pending_features: Vec::new(),
            items,
            label,
        };

        if let Some(parent) = parent {
            let mut new_nodes = FxHashMap::default();
            for nx in parent.g.node_indices() {
                let new_nx = match &parent.g[nx] {
                    Node::Item(i) => Some(s.add_item(i.clone()).into_untyped()),
                    Node::PendingFeature(f) | Node::ParentFeature(f) => {
                        Some(s.g.add_node(Node::ParentFeature(f.clone())))
                    }
                    _ => None,
                };
                if let Some(new_nx) = new_nx {
                    new_nodes.insert(nx, new_nx);
                }
            }
            for ex in parent.g.edge_indices() {
                let (a, b) = parent.g.edge_endpoints(ex).expect("must exist");
                let weight = parent.g.edge_weight(ex).expect("must exist").clone();
                if new_nodes.contains_key(&a) && new_nodes.contains_key(&b) {
                    s.g.add_edge(new_nodes[&a], new_nodes[&b], weight);
                }
            }
        }
        s
    }

    fn item(&self, key: &ItemKey) -> Option<node::ItemNodeIndex> {
        match self.items.get(key) {
            Some(i) => Some(*i),
            None => {
                if let ItemKey::Path(path) = key {
                    // if any of the ancestor directory paths are actually
                    // symlinks, resolve them
                    for ancestor in path.ancestors().skip(1) {
                        if let Some(nx) = self.items.get(&ItemKey::Path(ancestor.into())) {
                            if let Item::Path(item::Path::Symlink { target, link }) = &self.g[*nx] {
                                // target may be a relative path, in which
                                // case we need to resolve it relative to
                                // the link
                                let target = match target.is_absolute() {
                                    true => target.clone(),
                                    false => link
                                        .parent()
                                        .expect("the link cannot itself be /")
                                        .join(target),
                                };
                                let new_path = target.join(path.strip_prefix(ancestor).expect(
                                    "ancestor path can definitely be stripped as a prefix",
                                ));
                                return self.item(&ItemKey::Path(new_path));
                            }
                        };
                    }
                    None
                } else {
                    None
                }
            }
        }
    }

    fn add_item(&mut self, item: Item) -> node::ItemNodeIndex {
        let key = item.key();
        *self
            .items
            .entry(key)
            .or_insert_with(|| self.g.add_node_typed(item))
    }

    pub fn add_feature(&mut self, feature: Feature) -> &mut Self {
        let feature_nx = self.g.add_node_typed(feature);

        // make sure all features are reachable from the root node
        self.g.update_edge(*self.root, *feature_nx, Edge::After);

        self.pending_features.push(feature_nx);
        self
    }

    pub fn build(mut self) -> Result<Graph> {
        // Add all the nodes provided by our pending features
        for (feature_nx, provides) in self
            .pending_features
            .par_iter()
            .map(|feature_nx| {
                let f = &self.g[*feature_nx];
                tracing::trace!("getting provides from {f:?}");
                let provides = f.provides().map_err(Error::Provides)?;
                Ok((*feature_nx, provides))
            })
            .collect::<Result<Vec<_>>>()?
        {
            for prov in provides {
                let prov_nx = self.add_item(prov);
                self.g.update_edge(*feature_nx, *prov_nx, Edge::Provides);
            }
        }

        let requires: Vec<_> = self
            .pending_features
            .par_iter()
            .map(|feature_nx| {
                let f = &self.g[*feature_nx];
                tracing::trace!("getting requires from {f:?}");
                let reqs = f.requires().map_err(Error::Requires)?;
                Ok((*feature_nx, reqs))
            })
            .collect::<Result<_>>()?;
        // Add all the ordered requirements edges after all provided items are
        // added so that we know if a required item is missing or just not
        // encountered yet.
        for (feature_nx, requires) in &requires {
            for req in requires.iter().filter(|r| r.ordered) {
                let req_nx = match self.item(&req.key) {
                    Some(nx) => nx.into_untyped(),
                    None => self.g.add_node(Node::MissingItem(req.key.clone())),
                };
                self.g
                    .update_edge(req_nx, **feature_nx, Edge::Requires(req.validator.clone()));
            }
        }

        // topo sort should not include edges within any parent features,
        // otherwise (un)ordered requirements could cause a cycle
        let mut topo_sortable_g = self.g.clone();
        topo_sortable_g.retain_edges(|g, ex| {
            if let Some((a, b)) = g.edge_endpoints(ex) {
                if let Node::ParentFeature(_) = &self.g[a] {
                    false
                } else if let Node::ParentFeature(_) = &self.g[b] {
                    false
                } else {
                    true
                }
            } else {
                true
            }
        });

        let topo = match petgraph::algo::toposort(&topo_sortable_g, None) {
            Ok(topo) => topo,
            Err(node_in_cycle) => {
                // there might be multiple cycles, we really only need to find
                // one though
                let mut cycle = vec![node_in_cycle.node_id()];
                let mut dfs = Dfs::new(&self.g, node_in_cycle.node_id());
                while let Some(nx) = dfs.next(&self.g) {
                    cycle.push(nx);
                    if self.g.neighbors(nx).contains(&node_in_cycle.node_id()) {
                        let mut cycle: Vec<_> = cycle
                            .into_iter()
                            .filter_map(|nx| match &self.g[nx] {
                                // only include pending features so that the user is not overwhelmed
                                Node::PendingFeature(f) => Some(f.clone()),
                                _ => None,
                            })
                            .collect();
                        // Rotate the cycle so that the "minimum value" feature
                        // is first. This is semantically meaningless but does
                        // guarantee that cycle error messages are deterministic
                        if let Some(min_idx) = cycle
                            .iter()
                            .enumerate()
                            .min_by(|(_, a), (_, b)| a.cmp(b))
                            .map(|(idx, _)| idx)
                        {
                            cycle.rotate_left(min_idx);
                        }
                        return Err(Error::Cycle(Cycle(cycle)));
                    }
                }
                unreachable!()
            }
        };

        // Now add unordered requirements. It's still useful to have an edge
        // between features when there are unordered requirements (eg:
        // validators take the same path), but we need to do it after the
        // topological sort to avoid any cycles. If this proves to be a bad idea
        // (unlikely, since we don't really care if it's truly a DAG after
        // toposort) we can come up with some other way to represent "weak"
        // edges like these.
        for (feature_nx, requires) in &requires {
            for req in requires.iter().filter(|r| !r.ordered) {
                let req_nx = match self.item(&req.key) {
                    Some(nx) => nx.into_untyped(),
                    None => self.g.add_node(Node::MissingItem(req.key.clone())),
                };
                self.g
                    .update_edge(req_nx, **feature_nx, Edge::Requires(req.validator.clone()));
            }
        }

        for nx in self.g.node_indices() {
            match &self.g[nx] {
                // If multiple nodes provide the same item, fail now
                Node::Item(item) => {
                    let features_that_provide: Vec<_> = self
                        .g
                        .neighbors_directed(nx, Direction::Incoming)
                        .filter_map(|nx| match &self.g[nx] {
                            Node::PendingFeature(f) => Some((true, f)),
                            Node::ParentFeature(f) => Some((false, f)),
                            _ => None,
                        })
                        .collect();

                    if features_that_provide.len() > 1 {
                        // [Item::Path] fully describes a directory, so if all the
                        // provided [Item]s are identical, it's not a conflict
                        if matches!(
                            item,
                            Item::Path(item::Path::Entry(item::FsEntry {
                                file_type: item::FileType::Directory,
                                ..
                            }))
                        ) {
                            tracing::trace!(
                                "directory item is provided by multiple features: {features_that_provide:?}"
                            );
                            let mut feature_items = FxHashSet::default();
                            for feat in features_that_provide
                                .iter()
                                // Only pending features need to be checked.
                                // Parent features have already been checked for
                                // conflicts in the layer they were defined in.
                                // It is impossible to re-analyze the
                                // ParentFeature at this point, because the
                                // input artifacts are not materialized when
                                // analyzing this layer
                                .filter_map(|(pending, feature)| match pending {
                                    true => Some(feature),
                                    false => None,
                                })
                            {
                                feature_items.extend(
                                    feat.provides()
                                        .map_err(Error::Provides)?
                                        .into_iter()
                                        .filter(|fi| fi.key() == item.key()),
                                );
                            }
                            if feature_items.len() > 1 {
                                tracing::error!(
                                    "directory items are not identical: {feature_items:?}"
                                );
                                return Err(Error::Conflict {
                                    item: item.clone(),
                                    features: features_that_provide
                                        .into_iter()
                                        .map(|(_, feature)| feature)
                                        .cloned()
                                        .collect(),
                                });
                            }
                        } else {
                            // Any other features with equivalent Data are allowed (we
                            // should prune it from the graph before passing it off to
                            // antlir2_compile, but that's an optimization for later).
                            // Anything that is not completely equivalent is considered
                            // a conflict and will cause a build failure.
                            if features_that_provide
                                .iter()
                                .map(|(_, feature)| feature)
                                .any(|f| f.data != features_that_provide[0].1.data)
                            {
                                return Err(Error::Conflict {
                                    item: item.clone(),
                                    features: features_that_provide
                                        .into_iter()
                                        .map(|(_, feature)| feature)
                                        .cloned()
                                        .collect(),
                                });
                            }
                        }
                    }
                }
                _ => (),
            }
        }
        // If there are any items that exist but fail validation rules, return
        // an Err now
        for edge in self.g.edge_indices() {
            match self.g.edge_weight(edge).expect("definitely exists") {
                Edge::Requires(validator) => {
                    let (item, feature) = self.g.edge_endpoints(edge).expect("definitely exists");
                    match &self.g[item] {
                        Node::Item(item) => {
                            let item = match item {
                                // if the item is a symlink (and we can find it
                                // in the graph), check validators against the
                                // target path, not the symlink itself
                                Item::Path(item::Path::Symlink { target, link }) => {
                                    // target may be a relative path, in which
                                    // case we need to resolve it relative to
                                    // the link
                                    let target = match target.is_absolute() {
                                        true => target.clone(),
                                        false => link
                                            .parent()
                                            .expect("the link cannot itself be /")
                                            .join(target),
                                    };
                                    match self.item(&ItemKey::Path(target)) {
                                        Some(target_item_nx) => &self.g[target_item_nx],
                                        None => item,
                                    }
                                }
                                _ => item,
                            };
                            if !validator.satisfies(item) {
                                return Err(Error::Unsatisfied {
                                    item: item.clone(),
                                    validator: validator.clone(),
                                    required_by: self.g[feature]
                                        .as_feature()
                                        .expect("endpoint is always feature")
                                        .clone(),
                                });
                            }
                        }
                        Node::MissingItem(key) => {
                            if *validator != Validator::DoesNotExist {
                                return Err(Error::MissingItem {
                                    key: key.clone(),
                                    required_by: self.g[feature]
                                        .as_feature()
                                        .expect("endpoint is always feature")
                                        .clone(),
                                });
                            }
                        }
                        _ => unreachable!("Requires edges cannot exist on anything but Items"),
                    }
                }
                _ => (),
            }
        }

        Ok(Graph {
            label: self.label,
            g: self.g,
            root: self.root,
            items: self.items,
            topo,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Graph {
    label: Label,
    g: StableGraph<Node, Edge>,
    root: node::RootNodeIndex,
    #[serde(with = "serde_items")]
    items: FxHashMap<ItemKey, node::ItemNodeIndex>,
    topo: Vec<NodeIndex<DefaultIx>>,
}

mod serde_items {
    use rustc_hash::FxHasher;
    use serde::de::Deserializer;
    use serde::ser::SerializeSeq;
    use serde::ser::Serializer;

    use super::*;

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> std::result::Result<FxHashMap<ItemKey, node::ItemNodeIndex>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<(ItemKey, node::ItemNodeIndex)> = Deserialize::deserialize(deserializer)?;
        let mut items = FxHashMap::with_capacity_and_hasher(
            vec.len(),
            std::hash::BuildHasherDefault::<FxHasher>::default(),
        );
        for (key, nx) in vec {
            items.insert(key, nx);
        }
        Ok(items)
    }

    pub fn serialize<S>(
        items: &FxHashMap<ItemKey, node::ItemNodeIndex>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(items.len()))?;
        for (key, nx) in items {
            seq.serialize_element(&(key, nx))?;
        }
        seq.end()
    }
}

impl Graph {
    pub fn builder(label: Label, parent: Option<Self>) -> GraphBuilder {
        GraphBuilder::new(label, parent)
    }

    /// Iterate over features in topographical order (dependencies sorted before the
    /// features that require them).
    pub fn pending_features(&self) -> impl Iterator<Item = &Feature> {
        self.topo.iter().filter_map(|nx| match &self.g[*nx] {
            Node::PendingFeature(f) => Some(f),
            _ => None,
        })
    }

    /// There are many image features that produce items that we cannot
    /// reasonably know ahead-of-time (for example, rpm installation). This
    /// method will add [Item] nodes for anything newly discovered in the
    /// filesystem and add it to the end of the graph since we don't know where
    /// it came from.
    pub fn populate_dynamic_items(&mut self, root: &Path) -> std::io::Result<()> {
        let mut seen_paths = FxHashSet::default();
        for entry in walkdir::WalkDir::new(root) {
            let entry = entry?;
            let path =
                Path::new("/").join(entry.path().strip_prefix(root).expect("this must succeed"));
            seen_paths.insert(path.clone());
            let key = ItemKey::Path(path.clone());
            if let std::collections::hash_map::Entry::Vacant(e) = self.items.entry(key) {
                let meta = entry.metadata()?;
                let path_item = if entry.path_is_symlink() {
                    let target = std::fs::read_link(entry.path())?;
                    item::Path::Symlink { target, link: path }
                } else {
                    item::Path::Entry(item::FsEntry {
                        path,
                        mode: meta.mode(),
                        file_type: meta.file_type().into(),
                    })
                };
                let nx = self.g.add_node_typed(Item::Path(path_item));
                e.insert(nx);
            }
        }
        // remove any items from the depgraph if they refer to paths that are no
        // longer in the actual filesystem
        self.g.retain_nodes(|graph, nx| match &graph[nx] {
            Node::Item(item) => {
                if let ItemKey::Path(p) = item.key() {
                    seen_paths.contains(p.as_path())
                } else {
                    true
                }
            }
            _ => true,
        });
        self.items.retain(|key, _val| match key {
            ItemKey::Path(p) => seen_paths.contains(p.as_path()),
            _ => true,
        });

        let passwd_path = root.join("etc/passwd");
        let passwd = if passwd_path.exists() {
            antlir2_users::passwd::EtcPasswd::parse(&std::fs::read_to_string(passwd_path)?)
                .map_err(std::io::Error::other)?
                .into_owned()
        } else {
            Default::default()
        };
        for user in passwd.into_records() {
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.items.entry(ItemKey::User(user.name.clone().into()))
            {
                let nx = self.g.add_node_typed(Item::User(item::User {
                    name: user.name.into(),
                }));
                e.insert(nx);
            }
        }
        let group_path = root.join("etc/group");
        let groups = if group_path.exists() {
            antlir2_users::group::EtcGroup::parse(&std::fs::read_to_string(group_path)?)
                .map_err(std::io::Error::other)?
                .into_owned()
        } else {
            Default::default()
        };
        for group in groups.into_records() {
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.items.entry(ItemKey::Group(group.name.clone().into()))
            {
                let nx = self.g.add_node_typed(Item::Group(item::Group {
                    name: group.name.into(),
                }));
                e.insert(nx);
            }
        }
        Ok(())
    }
}
