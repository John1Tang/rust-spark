//! Pipeline DAG — layer order via petgraph.
//!
//! [`PipelineDag`] wraps a `petgraph::DiGraph<String, ()>`. `toposort`
//! returns a `Vec<Vec<FlowId>>` where each inner `Vec` is a "layer":
//! flows in the same layer have no dependencies on each other and can
//! be executed in parallel.

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

use crate::spec::{Flow, Pipeline};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowId(pub NodeIndex);

/// A pipeline DAG, with flows as nodes and `depends_on` as directed
/// edges (parent → child).
#[derive(Debug, Clone)]
pub struct PipelineDag {
    pub graph: DiGraph<String, ()>,
    pub name_index: HashMap<String, NodeIndex>,
}

impl PipelineDag {
    pub fn from_pipeline(p: &Pipeline) -> Result<Self, crate::spec::SpecError> {
        let mut graph = DiGraph::<String, ()>::new();
        let mut name_index = HashMap::new();
        for f in &p.flows {
            let idx = graph.add_node(f.name.clone());
            name_index.insert(f.name.clone(), idx);
        }
        for f in &p.flows {
            let to = *name_index.get(&f.name).expect("just inserted");
            for dep in &f.depends_on {
                let from = *name_index
                    .get(dep)
                    .ok_or_else(|| crate::spec::SpecError::UnknownFlow(dep.clone()))?;
                graph.add_edge(from, to, ());
            }
        }
        // Catch cycles early with a clearer message.
        toposort(&graph, None).map_err(|e| {
            let node = graph[e.node_id()].clone();
            crate::spec::SpecError::Cycle(node)
        })?;
        Ok(Self { graph, name_index })
    }

    /// Topologically-sorted layers. Each inner vec is one "layer": all
    /// flows in a layer are independent of each other.
    pub fn layers(&self) -> Vec<Vec<FlowId>> {
        // Compute in-degree per node.
        let mut indeg: HashMap<NodeIndex, usize> =
            self.graph.node_indices().map(|n| (n, 0)).collect();
        for edge in self.graph.edge_references() {
            *indeg.get_mut(&edge.target()).unwrap() += 1;
        }
        // Iterate layer by layer.
        let mut layers: Vec<Vec<FlowId>> = Vec::new();
        let mut remaining: HashMap<NodeIndex, usize> = indeg.clone();
        while !remaining.is_empty() {
            let layer: Vec<NodeIndex> = remaining
                .iter()
                .filter_map(|(n, &d)| if d == 0 { Some(*n) } else { None })
                .collect();
            if layer.is_empty() {
                break;
            }
            for n in &layer {
                remaining.remove(n);
            }
            for n in &layer {
                for succ in self.graph.neighbors(*n) {
                    if let Some(d) = remaining.get_mut(&succ) {
                        *d = d.saturating_sub(1);
                    }
                }
            }
            layers.push(layer.into_iter().map(FlowId).collect());
        }
        layers
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn flow(&self, id: FlowId) -> Option<&str> {
        self.graph.node_weight(id.0).map(String::as_str)
    }
}

/// Look up a [`Flow`] by name in a [`Pipeline`]. Returns
/// `SpecError::UnknownFlow` if not present.
pub fn lookup<'a>(p: &'a Pipeline, name: &str) -> Result<&'a Flow, crate::spec::SpecError> {
    p.flows
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| crate::spec::SpecError::UnknownFlow(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Destination, FlowKind, Refresh, SourceSpec};

    fn p() -> Pipeline {
        Pipeline {
            pipeline: "p".into(),
            flows: vec![
                Flow {
                    name: "a".into(),
                    kind: FlowKind::StreamingTable,
                    depends_on: vec![],
                    source: SourceSpec::Sql,
                    query: "SELECT 1".into(),
                    refresh: Refresh::Full,
                    destination: Destination::File {
                        path: "/tmp/a".into(),
                    },
                },
                Flow {
                    name: "b".into(),
                    kind: FlowKind::MaterializedView,
                    depends_on: vec!["a".into()],
                    source: SourceSpec::Sql,
                    query: "SELECT 2".into(),
                    refresh: Refresh::Full,
                    destination: Destination::File {
                        path: "/tmp/b".into(),
                    },
                },
                Flow {
                    name: "c".into(),
                    kind: FlowKind::MaterializedView,
                    depends_on: vec!["a".into(), "b".into()],
                    source: SourceSpec::Sql,
                    query: "SELECT 3".into(),
                    refresh: Refresh::Full,
                    destination: Destination::File {
                        path: "/tmp/c".into(),
                    },
                },
            ],
        }
    }

    #[test]
    fn layers_topologically() {
        let d = PipelineDag::from_pipeline(&p()).unwrap();
        let layers = d.layers();
        assert_eq!(layers.len(), 3);
        assert_eq!(d.flow(layers[0][0]), Some("a"));
        assert_eq!(d.flow(layers[1][0]), Some("b"));
        assert_eq!(d.flow(layers[2][0]), Some("c"));
    }

    #[test]
    fn cycle_detected() {
        let mut pp = p();
        pp.flows[2].depends_on.push("a".into());
        pp.flows[0].depends_on.push("c".into());
        let r = PipelineDag::from_pipeline(&pp);
        assert!(matches!(r, Err(crate::spec::SpecError::Cycle(_))));
    }
}
