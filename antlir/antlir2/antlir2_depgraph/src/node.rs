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

use antlir2_features::Feature;
use petgraph::graph::DefaultIx;
use petgraph::graph::Graph;
use petgraph::graph::NodeIndex;
use serde::Deserialize;
use serde::Serialize;

use crate::item::Item;
use crate::item::ItemKey;
use crate::phase::Phase;
use crate::Edge;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub enum Node<'a> {
    /// A Feature that is to be compiled in this layer.
    PendingFeature(Feature<'a>),
    /// An item provided by the parent layer or a feature in this layer.
    Item(Item<'a>),
    /// An item that is required by a feature, but does not exist in the graph.
    /// Distinct from a [Node::Item] without any [Edge::Provides] edges because
    /// not enough information is known about missing dependencies to construct
    /// the full [Item].
    MissingItem(ItemKey<'a>),
    /// A Feature that was provided by a layer in the parent chain.
    ParentFeature(Feature<'a>),
    /// Start of a distinct phase of the image build process.
    PhaseStart(Phase),
    /// End of a distinct phase of the image build process. All features that
    /// are part of that phase will have an edge pointing to the end.
    PhaseEnd(Phase),
}

impl<'a> Node<'a> {
    pub(crate) fn as_feature(&self) -> Option<&Feature<'a>> {
        match &self {
            Self::PendingFeature(f) | Self::ParentFeature(f) => Some(f),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TypedNodeIndex<'a, N>(NodeIndex<DefaultIx>, PhantomData<&'a N>)
where
    N: NodeMapper<'a>;

impl<'a, N> TypedNodeIndex<'a, N>
where
    N: NodeMapper<'a>,
{
    pub fn into_untyped(self) -> NodeIndex<DefaultIx> {
        self.0
    }
}

impl<'a, N> Deref for TypedNodeIndex<'a, N>
where
    N: NodeMapper<'a>,
{
    type Target = NodeIndex<DefaultIx>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, N> Debug for TypedNodeIndex<'a, N>
where
    N: NodeMapper<'a>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}
impl<'a, N> Copy for TypedNodeIndex<'a, N> where N: NodeMapper<'a> {}
impl<'a, N> Clone for TypedNodeIndex<'a, N>
where
    N: NodeMapper<'a>,
{
    fn clone(&self) -> Self {
        Self(self.0, PhantomData)
    }
}

impl<'a, N> Index<TypedNodeIndex<'a, N>> for Graph<Node<'a>, Edge<'a>>
where
    N: NodeMapper<'a>,
{
    type Output = <N as NodeMapper<'a>>::Inner;

    fn index(&self, index: TypedNodeIndex<'a, N>) -> &Self::Output {
        N::as_inner(&self[index.0]).expect("TypedNodeIndex type did not match")
    }
}

impl<'a, N> IndexMut<TypedNodeIndex<'a, N>> for Graph<Node<'a>, Edge<'a>>
where
    N: NodeMapper<'a>,
{
    fn index_mut(&mut self, index: TypedNodeIndex<'a, N>) -> &mut Self::Output {
        N::as_inner_mut(&mut self[index.0]).expect("TypedNodeIndex type did not match")
    }
}

pub(crate) trait GraphExt<'a, N>
where
    N: NodeMapper<'a>,
{
    fn add_node_typed(&mut self, inner: N::Inner) -> TypedNodeIndex<'a, N>;
}

impl<'a, N> GraphExt<'a, N> for Graph<Node<'a>, Edge<'a>>
where
    N: NodeMapper<'a>,
{
    fn add_node_typed(&mut self, inner: N::Inner) -> TypedNodeIndex<'a, N> {
        let nx = self.add_node(N::into_node(inner));
        TypedNodeIndex(nx, PhantomData)
    }
}

pub(crate) trait NodeMapper<'a> {
    type Inner;

    fn into_node(i: Self::Inner) -> Node<'a>;
    fn as_inner<'b>(n: &'b Node<'a>) -> Option<&'b Self::Inner>;
    fn as_inner_mut<'b>(n: &'b mut Node<'a>) -> Option<&'b mut Self::Inner>;
}

macro_rules! node_mapper {
    ($mapper:ident, $variant:ident, $inner:ty) => {
        pub struct $mapper;

        impl<'a> NodeMapper<'a> for $mapper {
            type Inner = $inner;

            fn into_node(i: Self::Inner) -> Node<'a> {
                Node::$variant(i)
            }

            fn as_inner<'b>(node: &'b Node<'a>) -> Option<&'b Self::Inner> {
                match node {
                    Node::$variant(i) => Some(i),
                    _ => None,
                }
            }

            fn as_inner_mut<'b>(node: &'b mut Node<'a>) -> Option<&'b mut Self::Inner> {
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
        pub(crate) type $name<'a> = TypedNodeIndex<'a, $mapper>;

        node_mapper!($mapper, $variant, $inner);
    };

    ($name:ident, $mapper:ident, $variant:ident, $inner:ty) => {
        pub(crate) type $name = TypedNodeIndex<'static, $mapper>;

        node_mapper!($mapper, $variant, $inner);
    };
}

typed_node_index!(
    a,
    PendingFeatureNodeIndex,
    PendingFeatureNodeIndexMapper,
    PendingFeature,
    antlir2_features::Feature<'a>
);

typed_node_index!(a, ItemNodeIndex, ItemNodeIndexMapper, Item, Item<'a>);
typed_node_index!(
    a,
    MissingItemNodeIndex,
    MissingItemNodeIndexMapper,
    MissingItem,
    ItemKey<'a>
);

typed_node_index!(
    a,
    ParentFeatureNodeIndex,
    ParentFeatureNodeIndexMapper,
    ParentFeature,
    antlir2_features::Feature<'a>
);

typed_node_index!(
    PhaseStartNodeIndex,
    PhaseStartNodeIndexMapper,
    PhaseStart,
    Phase
);
typed_node_index!(PhaseEndNodeIndex, PhaseEndNodeIndexMapper, PhaseEnd, Phase);
