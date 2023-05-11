/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_other)]

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fmt::Display;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use antlir2_features::Feature;
use buck_label::Label;
use itertools::Itertools;
use petgraph::graph::DefaultIx;
use petgraph::graph::DiGraph;
use petgraph::graph::NodeIndex;
use petgraph::visit::Dfs;
use petgraph::Direction;
use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use strum::IntoEnumIterator;

mod dot;
mod item;
use item::Item;
use item::ItemKey;
mod phase;
pub use phase::Phase;
mod requires_provides;
use requires_provides::FeatureExt as _;
use requires_provides::Validator;
mod node;
use node::GraphExt;
pub use node::Node;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub enum Edge<'a> {
    /// This feature is part of a phase
    PartOf,
    /// This feature provides an item
    Provides,
    /// This feature requires a provided item, and requires additional
    /// validation
    Requires(Validator<'a>),
    /// Simple ordering edge that does not require any additional checks
    After,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent, bound(deserialize = "'de: 'a"))]
pub struct Cycle<'a>(Vec<Feature<'a>>);

impl<'a> Display for Cycle<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for feature in &self.0 {
            writeln!(f, "  {feature:?}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "'de: 'a"))]
pub enum Error<'a> {
    #[error("cycle in dependency graph:\n{0}")]
    Cycle(Cycle<'a>),
    #[error("{item:?} is provided by multiple features: {features:#?}")]
    Conflict {
        item: Item<'a>,
        features: BTreeSet<Feature<'a>>,
    },
    #[error("{key:?} is required by {required_by:#?} but was never provided")]
    MissingItem {
        key: ItemKey<'a>,
        required_by: Feature<'a>,
    },
    #[error(
        "{item:?} does not satisfy the validation rules: {validator:?} as required by {required_by:#?}"
    )]
    Unsatisfied {
        item: Item<'a>,
        validator: Validator<'a>,
        required_by: Feature<'a>,
    },
    #[error("failure determining 'provides': {0}")]
    Provides(String),
}

pub type Result<'a, T> = std::result::Result<T, Error<'a>>;

#[derive(Debug)]
pub struct GraphBuilder<'a> {
    g: DiGraph<Node<'a>, Edge<'a>, DefaultIx>,
    root: node::PhaseStartNodeIndex,
    pending_features: Vec<node::PendingFeatureNodeIndex<'a>>,
    items: HashMap<ItemKey<'a>, node::ItemNodeIndex<'a>>,
    phases: BTreeMap<Phase, (node::PhaseStartNodeIndex, node::PhaseEndNodeIndex)>,
    rpm_feature: Option<node::PendingFeatureNodeIndex<'a>>,
    label: Label<'a>,
}

impl<'a> GraphBuilder<'a> {
    pub fn new(label: Label<'a>, parent: Option<Graph<'a>>) -> Self {
        let mut g = DiGraph::new();
        let mut items = HashMap::new();

        let phases: BTreeMap<_, _> = Phase::iter()
            .map(|p| (p, (g.add_node_typed(p), g.add_node_typed(p))))
            .collect();

        let root = phases[&Phase::Init].0;

        // Set up ordering for phases
        for ((a_start, a_end), (b_start, b_end)) in phases.values().tuple_windows() {
            g.update_edge(**a_start, **a_end, Edge::After);
            g.update_edge(**a_end, **b_start, Edge::After);
            g.update_edge(**b_start, **b_end, Edge::After);
        }

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

        let mut s = Self {
            g,
            root,
            pending_features: Vec::new(),
            items,
            phases,
            label,
            rpm_feature: None,
        };

        if let Some(parent) = parent {
            let mut new_nodes = HashMap::new();
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

    fn item(&self, key: &ItemKey<'_>) -> Option<node::ItemNodeIndex<'a>> {
        match self.items.get(key) {
            Some(i) => Some(*i),
            None => {
                if let ItemKey::Path(path) = key {
                    // if any of the ancestor directory paths are actually
                    // symlinks, resolve them
                    for ancestor in path.ancestors().skip(1) {
                        if let Some(nx) = self.items.get(&ItemKey::Path(Cow::Borrowed(ancestor))) {
                            if let Item::Path(item::Path::Symlink { target, link }) = &self.g[*nx] {
                                // target may be a relative path, in which
                                // case we need to resolve it relative to
                                // the link
                                let target = match target.is_absolute() {
                                    true => target.clone(),
                                    false => link
                                        .parent()
                                        .expect("the link cannot itself be /")
                                        .join(target)
                                        .into(),
                                };
                                let new_path = target.join(path.strip_prefix(ancestor).expect(
                                    "ancestor path can definitely be stripped as a prefix",
                                ));
                                return self.item(&ItemKey::Path(new_path.into()));
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

    fn add_item(&mut self, item: Item<'a>) -> node::ItemNodeIndex<'a> {
        let key = item.key();
        // If this Item undos another, it needs to be added to the graph on its
        // own. The previous Item will be left in the graph, but will be
        // overwritten in the items tracker map to this new node which is the
        // most recent version of that item
        if self.item(&key).is_some() && item.is_undo() {
            let nx = self.g.add_node_typed(item);
            self.g
                .add_edge(*self.items[&key], *nx, Edge::Requires(Validator::Exists));
            self.items.insert(key, nx);
            nx
        } else {
            *self
                .items
                .entry(key)
                .or_insert_with(|| self.g.add_node_typed(item))
        }
    }

    pub fn add_layer_dependency(&mut self, graph: Graph<'a>) -> &mut Self {
        self.add_item(Item::Layer(item::Layer {
            label: graph.label().clone(),
            graph,
        }));
        self
    }

    pub fn add_feature(&mut self, feature: Feature<'a>) -> &mut Self {
        // rpm features get merged into a single node so that the transaction management is easier
        let (phase, feature_nx) = if let antlir2_features::Data::Rpm(rpm) = feature.data {
            if let Some(nx) = self.rpm_feature {
                match &mut self.g[nx] {
                    Feature {
                        label: _,
                        data: antlir2_features::Data::Rpm(existing_feature),
                    } => existing_feature.items.extend(rpm.items),
                    _ => unreachable!("rpm_feature node is always an Rpm feature"),
                }
                return self;
            } else {
                let feature_nx = self.g.add_node_typed(Feature {
                    data: antlir2_features::Data::Rpm(rpm),
                    label: feature.label,
                });
                self.rpm_feature = Some(feature_nx);
                (Phase::OsPackage, feature_nx)
            }
        } else {
            let phase = Phase::for_feature(&feature);
            let feature_nx = self.g.add_node_typed(feature);
            (phase, feature_nx)
        };

        self.pending_features.push(feature_nx);

        // Insert edges so that features are in the right part of the graph wrt phases
        self.g
            .update_edge(*self.phases[&phase].0, *feature_nx, Edge::PartOf);
        self.g
            .update_edge(*feature_nx, *self.phases[&phase].1, Edge::After);

        self
    }

    pub fn to_dot<'b>(&'b self) -> dot::Dot<'a, 'b> {
        dot::Dot(&self.g)
    }

    pub fn build(mut self) -> Result<'a, Graph<'a>> {
        // Add all the nodes provided by our pending features
        for feature_nx in self.pending_features.clone() {
            let f = &self.g[feature_nx];
            let provides = f.provides().map_err(Error::Provides)?;
            for prov in provides {
                let prov_nx = self.add_item(prov);
                self.g.update_edge(*feature_nx, *prov_nx, Edge::Provides);
            }
        }

        // Add all the ordered requirements edges after all provided items are
        // added so that we know if a required item is missing or just not
        // encountered yet.
        for feature_nx in &self.pending_features {
            let f = &self.g[*feature_nx];
            for req in f.requires().into_iter().filter(|r| r.ordered) {
                let req_nx = match self.item(&req.key) {
                    Some(nx) => nx.into_untyped(),
                    None => {
                        let nx = self.g.add_node(Node::MissingItem(req.key.clone()));
                        nx
                    }
                };
                self.g
                    .update_edge(req_nx, **feature_nx, Edge::Requires(req.validator));
            }
        }

        let topo = match petgraph::algo::toposort(&self.g, None) {
            Ok(topo) => topo,
            Err(node_in_cycle) => {
                tracing::error!("cycle detected: dot: {:#?}", self.to_dot());
                // there might be multiple cycles, we really only need to find
                // one though
                let mut cycle = vec![node_in_cycle.node_id()];
                let mut dfs = Dfs::new(&self.g, node_in_cycle.node_id());
                while let Some(nx) = dfs.next(&self.g) {
                    cycle.push(nx);
                    if self.g.neighbors(nx).contains(&node_in_cycle.node_id()) {
                        let mut cycle: Vec<_> = cycle
                            .into_iter()
                            // only include the features so that it doesn't
                            // overwhelm the user
                            .filter_map(|nx| match &self.g[nx] {
                                Node::PendingFeature(f) => Some(f.clone()),
                                _ => None,
                            })
                            .collect();
                        // Rotate the cycle so that the "minimum value" feature
                        // is first. This is semantically meaningless but does
                        // guarantee that cycle error messages are deterministic
                        let min_index = cycle
                            .iter()
                            .enumerate()
                            .min_by(|(_, a), (_, b)| a.cmp(b))
                            .expect("there is always at least one element")
                            .0;
                        cycle.rotate_left(min_index);
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
        for feature_nx in &self.pending_features {
            let f = &self.g[*feature_nx];
            for req in f.requires().into_iter().filter(|r| !r.ordered) {
                let req_nx = match self.item(&req.key) {
                    Some(nx) => nx.into_untyped(),
                    None => {
                        let nx = self.g.add_node(Node::MissingItem(req.key.clone()));
                        nx
                    }
                };
                self.g
                    .update_edge(req_nx, **feature_nx, Edge::Requires(req.validator));
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
                            Node::PendingFeature(f) | Node::ParentFeature(f) => Some(f),
                            _ => None,
                        })
                        .collect();
                    // Feature's with equivalent Data are allowed (we should
                    // prune it from the graph before passing it off to
                    // antlir2_compile, but that's an optimization for later).
                    // Anything that is not completely equivalent is considered
                    // a conflict and will cause a build failure.
                    if features_that_provide.len() > 1
                        && features_that_provide
                            .iter()
                            .any(|f| f.data != features_that_provide[0].data)
                    {
                        return Err(Error::Conflict {
                            item: item.clone(),
                            features: features_that_provide.into_iter().cloned().collect(),
                        });
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
                                            .join(target)
                                            .into(),
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
            end: self.phases[&Phase::End],
        })
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Graph<'a> {
    #[serde(borrow)]
    label: Label<'a>,
    #[serde(borrow)]
    g: DiGraph<Node<'a>, Edge<'a>>,
    root: node::PhaseStartNodeIndex,
    #[serde_as(as = "Vec<(_, _)>")]
    items: HashMap<ItemKey<'a>, node::ItemNodeIndex<'a>>,
    topo: Vec<NodeIndex<DefaultIx>>,
    end: (node::PhaseStartNodeIndex, node::PhaseEndNodeIndex),
}

impl<'a> Graph<'a> {
    pub fn builder(label: Label<'a>, parent: Option<Self>) -> GraphBuilder<'a> {
        GraphBuilder::new(label, parent)
    }

    pub fn label(&self) -> &Label<'a> {
        &self.label
    }

    pub fn to_dot<'b>(&'b self) -> dot::Dot<'a, 'b> {
        dot::Dot(&self.g)
    }

    /// Iterate over features in topographical order (dependencies sorted before the
    /// features that require them).
    pub fn pending_features(&self) -> impl Iterator<Item = &Feature<'a>> {
        self.topo.iter().filter_map(|nx| match &self.g[*nx] {
            Node::PendingFeature(f) => Some(f),
            _ => None,
        })
    }

    pub(crate) fn get_item(&self, key: &ItemKey<'_>) -> Option<&Item<'a>> {
        match self.items.get(key) {
            Some(nx) => Some(&self.g[*nx]),
            None => None,
        }
    }

    /// There are many image features that produce items that we cannot
    /// reasonably know ahead-of-time (for example, rpm installation). This
    /// method will add [Item] nodes for anything newly discovered in the
    /// filesystem and add it to the end of the graph since we don't know where
    /// it came from.
    pub fn populate_dynamic_items(&mut self, root: &Path) -> std::io::Result<()> {
        for entry in walkdir::WalkDir::new(root) {
            let entry = entry?;
            let path =
                Path::new("/").join(entry.path().strip_prefix(root).expect("this must succeed"));
            let key = ItemKey::Path(path.clone().into());
            if let std::collections::hash_map::Entry::Vacant(e) = self.items.entry(key) {
                let meta = entry.metadata()?;
                let path_item = if entry.path_is_symlink() {
                    let target = std::fs::read_link(entry.path())?;
                    item::Path::Symlink {
                        target: target.into(),
                        link: path.into(),
                    }
                } else {
                    item::Path::Entry(item::FsEntry {
                        path: path.into(),
                        mode: meta.mode(),
                        file_type: meta.file_type().into(),
                    })
                };
                let nx = self.g.add_node_typed(Item::Path(path_item));
                e.insert(nx);
                self.g.update_edge(*self.end.0, *nx, Edge::PartOf);
                self.g.update_edge(*nx, *self.end.1, Edge::After);
            }
        }
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
                self.items.entry(ItemKey::User(user.name.clone()))
            {
                let nx = self
                    .g
                    .add_node_typed(Item::User(item::User { name: user.name }));
                e.insert(nx);
                self.g.update_edge(*self.end.0, *nx, Edge::PartOf);
                self.g.update_edge(*nx, *self.end.1, Edge::After);
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
                self.items.entry(ItemKey::Group(group.name.clone()))
            {
                let nx = self
                    .g
                    .add_node_typed(Item::Group(item::Group { name: group.name }));
                e.insert(nx);
                self.g.update_edge(*self.end.0, *nx, Edge::PartOf);
                self.g.update_edge(*nx, *self.end.1, Edge::After);
            }
        }
        Ok(())
    }
}
