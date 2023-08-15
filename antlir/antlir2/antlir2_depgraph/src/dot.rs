/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::fmt::Formatter;

use petgraph::stable_graph::StableGraph;

use crate::Edge;
use crate::Node;

/// Better-looking dot rendering
pub struct Dot<'a, 'b>(pub(crate) &'b StableGraph<Node<'a>, Edge<'a>>);

fn node_color(node: &Node) -> &'static str {
    match node {
        Node::PendingFeature(_) => "white",
        Node::Item(_) => "ivory",
        Node::MissingItem(_) => "red",
        Node::ParentFeature(_) => "darkseagreen",
        Node::Root(_) => "grey75",
    }
}

fn debug_node(node: &Node<'_>, alternate: bool) -> String {
    let s = match (node, alternate) {
        (Node::Root(_), _) => "Root".to_owned(),
        (_, true) => format!("{:#?}\n", node),
        (_, false) => format!("{:?}", node),
    };
    s.replace('\n', "\\l").replace('"', "\\\"")
}

impl<'a, 'b> Debug for Dot<'a, 'b> {
    fn fmt(&self, fmt: &mut Formatter) -> std::fmt::Result {
        writeln!(fmt, "digraph {{")?;
        writeln!(fmt, "  graph [nodesep=\"1\", ranksep=\"2\"]")?;
        writeln!(fmt, "  splines = \"off\"")?;
        writeln!(fmt, "  node[shape=box]")?;
        for nx in self.0.node_indices() {
            let node = &self.0[nx];
            writeln!(
                fmt,
                "  {} [label=\"{}\", style=\"filled\", fillcolor=\"{}\"]",
                nx.index(),
                debug_node(node, fmt.alternate()),
                node_color(node),
            )?;
        }
        for ex in self.0.edge_indices() {
            let (a, b) = self.0.edge_endpoints(ex).expect("definitely exists");
            let weight = self.0.edge_weight(ex).expect("definitely exists");
            writeln!(
                fmt,
                "  {} -> {} [xlabel = \"{:?}\"]",
                a.index(),
                b.index(),
                weight
            )?;
        }
        writeln!(fmt, "}}")
    }
}
