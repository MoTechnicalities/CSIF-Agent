use csif_core::{contradiction_threshold, phase_distance, wrap_pi};
use rwif_core::{Edge, RWIFCrystal};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub edge_id: String,
    pub source: String,
    pub target: String,
    pub phase: f64,
    pub sigma: f64,
}

#[derive(Clone, Debug)]
pub struct ConflictPathTrace {
    pub source: String,
    pub target: String,
    pub path_a: Vec<String>,
    pub path_b: Vec<String>,
    pub phase_a: f64,
    pub phase_b: f64,
    pub residual: f64,
}

#[derive(Clone, Debug)]
pub struct PhaseGraph {
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Debug)]
struct PathRecord {
    nodes: Vec<String>,
    steps: Vec<(usize, i8)>,
}

impl PhaseGraph {
    pub fn from_crystal(crystal: &RWIFCrystal) -> Self {
        let mut ids: Vec<&String> = crystal.edges.keys().collect();
        ids.sort();

        let mut edges = Vec::with_capacity(ids.len());

        for id in ids {
            if let Some(edge) = crystal.edges.get(id) {
                let (phase, sigma) = edge
                    .trajectory
                    .last()
                    .map(|e| (e.phase, e.sigma))
                    .unwrap_or((0.0, 0.1));

                edges.push(GraphEdge {
                    edge_id: edge.edge_id.clone(),
                    source: edge.source.clone(),
                    target: edge.target.clone(),
                    phase,
                    sigma,
                });
            }
        }

        Self { edges }
    }

    pub fn max_multipath_conflict(&self) -> f64 {
        self.conflict_traces()
            .iter()
            .map(|t| t.residual)
            .fold(0.0_f64, f64::max)
    }

    pub fn conflict_traces(&self) -> Vec<ConflictPathTrace> {
        let nodes: Vec<String> = self.unique_nodes().into_iter().collect();
        let mut traces = Vec::new();

        for source in &nodes {
            for target in &nodes {
                if source == target {
                    continue;
                }

                let paths = self.all_simple_paths(source, target, nodes.len() + 1);
                if paths.len() < 2 {
                    continue;
                }

                for i in 0..paths.len() {
                    for j in (i + 1)..paths.len() {
                        let phase_a = self.path_phase(&paths[i]);
                        let phase_b = self.path_phase(&paths[j]);
                        traces.push(ConflictPathTrace {
                            source: source.clone(),
                            target: target.clone(),
                            path_a: paths[i].nodes.clone(),
                            path_b: paths[j].nodes.clone(),
                            phase_a,
                            phase_b,
                            residual: phase_distance(phase_a, phase_b),
                        });
                    }
                }
            }
        }

        traces.sort_by(|a, b| {
            b.residual
                .partial_cmp(&a.residual)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.source.cmp(&b.source))
                .then_with(|| a.target.cmp(&b.target))
        });

        traces
    }

    fn unique_nodes(&self) -> BTreeSet<String> {
        let mut nodes = BTreeSet::new();
        for edge in &self.edges {
            nodes.insert(edge.source.clone());
            nodes.insert(edge.target.clone());
        }
        nodes
    }

    fn all_simple_paths(&self, source: &str, target: &str, max_depth: usize) -> Vec<PathRecord> {
        let mut paths = Vec::new();
        let mut stack: Vec<(String, Vec<String>, Vec<(usize, i8)>)> =
            vec![(source.to_string(), vec![source.to_string()], Vec::new())];

        while let Some((current, node_path, step_path)) = stack.pop() {
            if current == target && node_path.len() > 1 {
                paths.push(PathRecord {
                    nodes: node_path.clone(),
                    steps: step_path.clone(),
                });
            }

            if node_path.len() >= max_depth {
                continue;
            }

            for (idx, edge) in self.edges.iter().enumerate() {
                let candidate = if edge.source == current {
                    Some((edge.target.as_str(), 1_i8))
                } else if edge.target == current {
                    Some((edge.source.as_str(), -1_i8))
                } else {
                    None
                };

                if let Some((next_node, sign)) = candidate {
                    if node_path.iter().any(|n| n == next_node) {
                        continue;
                    }

                    let mut next_nodes = node_path.clone();
                    next_nodes.push(next_node.to_string());

                    let mut next_steps = step_path.clone();
                    next_steps.push((idx, sign));

                    stack.push((next_node.to_string(), next_nodes, next_steps));
                }
            }
        }

        paths
    }

    fn path_phase(&self, path: &PathRecord) -> f64 {
        path.steps.iter().fold(0.0_f64, |acc, (idx, sign)| {
            let edge = &self.edges[*idx];
            let signed = if *sign == 1 {
                edge.phase
            } else {
                wrap_pi(-edge.phase)
            };
            wrap_pi(acc + signed)
        })
    }
}

#[derive(Debug)]
pub struct GuardDecision {
    pub max_residual: f64,
    pub threshold: f64,
    pub intercepted: bool,
}

pub fn evaluate_contradiction(baseline: &Edge, candidate: &Edge, c: f64) -> GuardDecision {
    let baseline_reverse = wrap_pi(-baseline.theta);
    let residual = phase_distance(baseline_reverse, candidate.theta);
    let mean_sigma = (baseline.sigma + candidate.sigma) / 2.0;
    let threshold = contradiction_threshold(mean_sigma, c);

    GuardDecision {
        max_residual: residual,
        threshold,
        intercepted: residual > threshold,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::f64::consts::PI;
    use rwif_core::{PhaseEvent, Provenance, RWIFEdge, RWIFNode};

    fn crystal_with_three_paths(direct_phase: f64) -> RWIFCrystal {
        let mut nodes = HashMap::new();
        nodes.insert(
            "A".to_string(),
            RWIFNode {
                node_id: "A".to_string(),
                label: "A".to_string(),
            },
        );
        nodes.insert(
            "B".to_string(),
            RWIFNode {
                node_id: "B".to_string(),
                label: "B".to_string(),
            },
        );
        nodes.insert(
            "C".to_string(),
            RWIFNode {
                node_id: "C".to_string(),
                label: "C".to_string(),
            },
        );

        let mk_event = |phase: f64| PhaseEvent {
            timestamp: "2026-05-18T00:00:00Z".parse().unwrap(),
            phase,
            sigma: 0.02,
            source: Provenance {
                source_type: "test".to_string(),
                source_id: "seed".to_string(),
            },
        };

        let mut edges = HashMap::new();
        edges.insert(
            "e_ab".to_string(),
            RWIFEdge {
                edge_id: "e_ab".to_string(),
                source: "A".to_string(),
                relation: "rel".to_string(),
                target: "B".to_string(),
                lobe: "English".to_string(),
                trajectory: vec![mk_event(0.0)],
            },
        );
        edges.insert(
            "e_bc".to_string(),
            RWIFEdge {
                edge_id: "e_bc".to_string(),
                source: "B".to_string(),
                relation: "rel".to_string(),
                target: "C".to_string(),
                lobe: "English".to_string(),
                trajectory: vec![mk_event(0.0)],
            },
        );
        edges.insert(
            "e_ac".to_string(),
            RWIFEdge {
                edge_id: "e_ac".to_string(),
                source: "A".to_string(),
                relation: "rel".to_string(),
                target: "C".to_string(),
                lobe: "English".to_string(),
                trajectory: vec![mk_event(direct_phase)],
            },
        );

        RWIFCrystal {
            id: "crystal".to_string(),
            nodes,
            edges,
        }
    }

    #[test]
    fn coherent_graph_max_conflict_is_zero() {
        let crystal = crystal_with_three_paths(0.0);
        let graph = PhaseGraph::from_crystal(&crystal);
        let max = graph.max_multipath_conflict();
        assert!(max.abs() < 1e-12, "expected near zero, got {}", max);
    }

    #[test]
    fn contradictory_graph_max_conflict_is_pi() {
        let crystal = crystal_with_three_paths(PI);
        let graph = PhaseGraph::from_crystal(&crystal);
        let max = graph.max_multipath_conflict();
        assert!((max - PI).abs() < 1e-9, "expected PI, got {}", max);
    }
}
