/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ops::Index;
use std::ops::IndexMut;

use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_features::Feature;
use petgraph::graph::DefaultIx;
use petgraph::graph::NodeIndex;
use petgraph::stable_graph::StableGraph;
use serde::Deserialize;
use serde::Serialize;

use crate::Edge;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Node {
    /// A Feature that is to be compiled in this layer.
    PendingFeature(Feature),
    /// An item provided by the parent layer or a feature in this layer.
    Item(Item),
    /// An item that is required by a feature, but does not exist in the graph.
    /// Distinct from a [Node::Item] without any [Edge::Provides] edges because
    /// not enough information is known about missing dependencies to construct
    /// the full [Item].
    MissingItem(ItemKey),
    /// A Feature that was provided by a layer in the parent chain.
    ParentFeature(Feature),
    /// Root node, starting point for image build
    Root(()),
}

impl Node {
    pub(crate) fn as_feature(&self) -> Option<&Feature> {
        match &self {
            Self::PendingFeature(f) | Self::ParentFeature(f) => Some(f),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TypedNodeIndex<N>(NodeIndex<DefaultIx>, PhantomData<N>)
where
    N: NodeMapper;

impl<N> TypedNodeIndex<N>
where
    N: NodeMapper,
{
    pub fn into_untyped(self) -> NodeIndex<DefaultIx> {
        self.0
    }
}

impl<N> Deref for TypedNodeIndex<N>
where
    N: NodeMapper,
{
    type Target = NodeIndex<DefaultIx>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<N> Debug for TypedNodeIndex<N>
where
    N: NodeMapper,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}
impl<N> Copy for TypedNodeIndex<N> where N: NodeMapper {}
impl<N> Clone for TypedNodeIndex<N>
where
    N: NodeMapper,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<N> Index<TypedNodeIndex<N>> for StableGraph<Node, Edge>
where
    N: NodeMapper,
{
    type Output = <N as NodeMapper>::Inner;

    fn index(&self, index: TypedNodeIndex<N>) -> &Self::Output {
        N::as_inner(&self[index.0]).expect("TypedNodeIndex type did not match")
    }
}

impl<N> IndexMut<TypedNodeIndex<N>> for StableGraph<Node, Edge>
where
    N: NodeMapper,
{
    fn index_mut(&mut self, index: TypedNodeIndex<N>) -> &mut Self::Output {
        N::as_inner_mut(&mut self[index.0]).expect("TypedNodeIndex type did not match")
    }
}

pub(crate) trait GraphExt<N>
where
    N: NodeMapper,
{
    fn add_node_typed(&mut self, inner: N::Inner) -> TypedNodeIndex<N>;
}

impl<N> GraphExt<N> for StableGraph<Node, Edge>
where
    N: NodeMapper,
{
    fn add_node_typed(&mut self, inner: N::Inner) -> TypedNodeIndex<N> {
        let nx = self.add_node(N::into_node(inner));
        TypedNodeIndex(nx, PhantomData)
    }
}

pub(crate) trait NodeMapper {
    type Inner;

    fn into_node(i: Self::Inner) -> Node;
    fn as_inner<'b>(n: &'b Node) -> Option<&'b Self::Inner>;
    fn as_inner_mut<'b>(n: &'b mut Node) -> Option<&'b mut Self::Inner>;
}

macro_rules! node_mapper {
    ($mapper:ident, $variant:ident, $inner:ty) => {
        pub struct $mapper;

        impl NodeMapper for $mapper {
            type Inner = $inner;

            fn into_node(i: Self::Inner) -> Node {
                Node::$variant(i)
            }

            fn as_inner<'b>(node: &'b Node) -> Option<&'b Self::Inner> {
                match node {
                    Node::$variant(i) => Some(i),
                    _ => None,
                }
            }

            fn as_inner_mut<'b>(node: &'b mut Node) -> Option<&'b mut Self::Inner> {
                match node {
                    Node::$variant(i) => Some(i),
                    _ => None,
                }
            }
        }
    };
}

macro_rules! typed_node_index {
    (a, $name:ident, $mapper:ident, $variant:ident, $inner:ty) => {
        #[allow(dead_code)]
        pub(crate) type $name = TypedNodeIndex<$mapper>;

        node_mapper!($mapper, $variant, $inner);
    };

    ($name:ident, $mapper:ident, $variant:ident, $inner:ty) => {
        #[allow(dead_code)]
        pub(crate) type $name = TypedNodeIndex<$mapper>;

        node_mapper!($mapper, $variant, $inner);
    };
}

typed_node_index!(
    a,
    PendingFeatureNodeIndex,
    PendingFeatureNodeIndexMapper,
    PendingFeature,
    antlir2_features::Feature
);

typed_node_index!(a, ItemNodeIndex, ItemNodeIndexMapper, Item, Item);
typed_node_index!(
    a,
    MissingItemNodeIndex,
    MissingItemNodeIndexMapper,
    MissingItem,
    ItemKey
);

typed_node_index!(
    a,
    ParentFeatureNodeIndex,
    ParentFeatureNodeIndexMapper,
    ParentFeature,
    antlir2_features::Feature
);

typed_node_index!(RootNodeIndex, RootNodeIndexMapper, Root, ());
