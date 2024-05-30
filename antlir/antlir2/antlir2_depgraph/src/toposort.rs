/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_features::Feature;
use fxhash::FxHashMap;
use itertools::Itertools;
use petgraph::graph::DiGraph;
use petgraph::visit::Dfs;
use rusqlite::Connection;

use crate::error::ContextExt;
use crate::Cycle;
use crate::Error;
use crate::Result;

/// Topologically sort pending features in dependency order
pub(crate) fn toposort(db: &Connection) -> Result<Vec<Feature>> {
    let mut nodes: FxHashMap<_, _> = Default::default();
    let mut graph: DiGraph<i64, ()> = DiGraph::new();
    // All we have to do is find ordered feature dependencies (and features with
    // no dependencies). We've already validated that the graph does not have
    // any missing dependencies or conflicts.
    for row in db
        .prepare(
            r#"
            SELECT
                f.id AS id,
                CASE WHEN r.ordered THEN f2.id ELSE NULL END AS requires_feature
            FROM feature f
            LEFT JOIN requires r ON f.id = r.feature
            LEFT JOIN item i ON i.key = r.item_key
            LEFT JOIN provides p ON i.id = p.item
            LEFT JOIN feature f2 ON p.feature = f2.id
            WHERE
                f.pending=1
            ORDER BY id ASC
        "#,
        )
        .context("while preparing toposort query")?
        .query_map([], |row| {
            let feature: i64 = row.get("id")?;
            let requires_feature: Option<i64> = row.get("requires_feature")?;
            Ok((feature, requires_feature))
        })
        .context("while executing toposort query")?
    {
        let (feature, requires_feature) = row?;
        let feature = *nodes
            .entry(feature)
            .or_insert_with(|| graph.add_node(feature));
        if let Some(requires_feature) = requires_feature {
            let requires_feature = *nodes
                .entry(requires_feature)
                .or_insert_with(|| graph.add_node(requires_feature));
            graph.update_edge(requires_feature, feature, ());
        }
    }
    let mut features: FxHashMap<_, _> = db
        .prepare("SELECT id, value FROM feature WHERE pending=1")
        .context("while preparing toposort load query")?
        .query_and_then([], |row| {
            let id: i64 = row.get("id")?;
            let feature: Feature = serde_json::from_str(
                row.get_ref("value")?
                    .as_str()
                    .map_err(rusqlite::Error::from)?,
            )
            .map_err(Error::GraphSerde)?;
            Ok((id, feature))
        })
        .context("while executing toposort load query")?
        .collect::<Result<_>>()?;
    match petgraph::algo::toposort(&graph, None) {
        Ok(sorted) => Ok(sorted
            .into_iter()
            .filter_map(|nx| features.remove(&graph[nx]))
            .collect()),
        Err(node_in_cycle) => {
            // there might be multiple cycles, we really only need to find
            // one though
            let mut cycle = vec![node_in_cycle.node_id()];
            let mut dfs = Dfs::new(&graph, node_in_cycle.node_id());
            while let Some(nx) = dfs.next(&graph) {
                cycle.push(nx);
                if graph.neighbors(nx).contains(&node_in_cycle.node_id()) {
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
                    return Err(Error::Cycle(Cycle(
                        cycle
                            .into_iter()
                            .filter_map(|nx| features.remove(&graph[nx]))
                            .collect(),
                    )));
                }
            }
            unreachable!("DFS will have let us complete the cycle")
        }
    }
}
