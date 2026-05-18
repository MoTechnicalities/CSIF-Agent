use chrono::Utc;
use csif_core::{circular_mean, contradiction_threshold, normalized_resonance, phase_distance, wrap_pi};
use rwif_core::{PhaseEvent, Provenance, RWIFCrystal, RWIFEdge};

#[derive(Debug)]
pub struct SyncReport {
    pub consensus_phase: f64,
    pub max_deviation: f64,
    pub converged: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncVerdict {
    Skip,
    Nudge,
    Reject,
}

#[derive(Clone, Debug)]
pub struct EdgeUpdate {
    pub edge_id: String,
    pub source: String,
    pub relation: String,
    pub target: String,
    pub lobe: String,
    pub incoming_phase: f64,
    pub incoming_sigma: f64,
}

#[derive(Clone, Debug)]
pub struct SyncDelta {
    pub source_agent: String,
    pub crystal_id: String,
    pub edge_updates: Vec<EdgeUpdate>,
}

pub fn consensus_report(agent_phases: &[f64], convergence_epsilon: f64) -> Option<SyncReport> {
    let consensus = circular_mean(agent_phases)?;

    let max_dev = agent_phases
        .iter()
        .map(|p| phase_distance(*p, consensus))
        .fold(0.0_f64, f64::max);

    Some(SyncReport {
        consensus_phase: consensus,
        max_deviation: max_dev,
        converged: max_dev <= convergence_epsilon,
    })
}

pub fn evaluate_delta(local_crystal: &RWIFCrystal, delta: &SyncDelta) -> (SyncVerdict, Option<f64>) {
    let mut saw_nudge = false;
    let mut nudge_phase = None;

    for update in &delta.edge_updates {
        if let Some(local_edge_id) = find_matching_edge_id(local_crystal, update) {
            let local_edge = &local_crystal.edges[&local_edge_id];
            let (local_phase, local_sigma) = latest_phase_sigma(local_edge);

            let residual = phase_distance(update.incoming_phase, local_phase);
            let threshold = contradiction_threshold((local_sigma + update.incoming_sigma) / 2.0, 0.5);

            if residual > threshold {
                return (SyncVerdict::Reject, None);
            }

            let rn = normalized_resonance(update.incoming_phase, local_phase);

            if rn < 0.05 {
                continue;
            }

            if rn < 0.5 {
                saw_nudge = true;
                let nudged = nudge_phase_toward(local_phase, update.incoming_phase, 0.5);
                nudge_phase = Some(nudged);
            } else {
                return (SyncVerdict::Reject, None);
            }
        } else {
            saw_nudge = true;
            nudge_phase = Some(update.incoming_phase);
        }
    }

    if saw_nudge {
        (SyncVerdict::Nudge, nudge_phase)
    } else {
        (SyncVerdict::Skip, None)
    }
}

pub fn apply_nudge(local_crystal: &mut RWIFCrystal, delta: &SyncDelta, nudge_rate: f64) {
    for update in &delta.edge_updates {
        if let Some(local_edge_id) = find_matching_edge_id(local_crystal, update) {
            if let Some(local_edge) = local_crystal.edges.get(&local_edge_id).cloned() {
                let (local_phase, local_sigma) = latest_phase_sigma(&local_edge);
                let nudged = nudge_phase_toward(local_phase, update.incoming_phase, nudge_rate);
                let sigma = (local_sigma + update.incoming_sigma) / 2.0;

                let event = PhaseEvent {
                    timestamp: Utc::now(),
                    phase: nudged,
                    sigma,
                    source: Provenance {
                        source_type: "sync_nudge".to_string(),
                        source_id: delta.source_agent.clone(),
                    },
                };

                let _ = local_crystal.append_trajectory(&local_edge_id, event);
            }
        } else {
            let event = PhaseEvent {
                timestamp: Utc::now(),
                phase: update.incoming_phase,
                sigma: update.incoming_sigma,
                source: Provenance {
                    source_type: "sync_insert".to_string(),
                    source_id: delta.source_agent.clone(),
                },
            };

            local_crystal.edges.insert(
                update.edge_id.clone(),
                RWIFEdge {
                    edge_id: update.edge_id.clone(),
                    source: update.source.clone(),
                    relation: update.relation.clone(),
                    target: update.target.clone(),
                    lobe: update.lobe.clone(),
                    trajectory: vec![event],
                },
            );
        }
    }
}

fn latest_phase_sigma(edge: &RWIFEdge) -> (f64, f64) {
    edge.trajectory
        .last()
        .map(|e| (e.phase, e.sigma))
        .unwrap_or((0.0, 0.1))
}

fn nudge_phase_toward(local_phase: f64, incoming_phase: f64, nudge_rate: f64) -> f64 {
    let delta = wrap_pi(incoming_phase - local_phase);
    wrap_pi(local_phase + nudge_rate * delta)
}

fn find_matching_edge_id(local_crystal: &RWIFCrystal, update: &EdgeUpdate) -> Option<String> {
    if local_crystal.edges.contains_key(&update.edge_id) {
        return Some(update.edge_id.clone());
    }

    local_crystal
        .edges
        .iter()
        .find(|(_, edge)| {
            edge.source == update.source
                && edge.relation == update.relation
                && edge.target == update.target
        })
        .map(|(id, _)| id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::f64::consts::PI;
    use rwif_core::RWIFNode;

    fn base_crystal(local_phase: f64, local_sigma: f64) -> RWIFCrystal {
        let mut nodes = HashMap::new();
        nodes.insert(
            "light".to_string(),
            RWIFNode {
                node_id: "light".to_string(),
                label: "light".to_string(),
            },
        );
        nodes.insert(
            "darkness".to_string(),
            RWIFNode {
                node_id: "darkness".to_string(),
                label: "darkness".to_string(),
            },
        );

        let mut edges = std::collections::HashMap::new();
        edges.insert(
            "edge-local".to_string(),
            RWIFEdge {
                edge_id: "edge-local".to_string(),
                source: "light".to_string(),
                relation: "dispels".to_string(),
                target: "darkness".to_string(),
                lobe: "English".to_string(),
                trajectory: vec![PhaseEvent {
                    timestamp: "2026-05-18T00:00:00Z".parse().unwrap(),
                    phase: local_phase,
                    sigma: local_sigma,
                    source: Provenance {
                        source_type: "seed".to_string(),
                        source_id: "baseline".to_string(),
                    },
                }],
            },
        );

        RWIFCrystal {
            id: "c-sync".to_string(),
            nodes,
            edges,
        }
    }

    fn incoming(phase: f64, sigma: f64) -> SyncDelta {
        SyncDelta {
            source_agent: "agent-a".to_string(),
            crystal_id: "c-sync".to_string(),
            edge_updates: vec![EdgeUpdate {
                edge_id: "edge-remote".to_string(),
                source: "light".to_string(),
                relation: "dispels".to_string(),
                target: "darkness".to_string(),
                lobe: "English".to_string(),
                incoming_phase: phase,
                incoming_sigma: sigma,
            }],
        }
    }

    #[test]
    fn scenario_skip() {
        let crystal = base_crystal(0.0, 0.02);
        let delta = incoming(0.0, 0.02);
        let (verdict, nudged) = evaluate_delta(&crystal, &delta);
        assert_eq!(verdict, SyncVerdict::Skip);
        assert!(nudged.is_none());
    }

    #[test]
    fn scenario_nudge() {
        let mut crystal = base_crystal(0.30, 0.10);
        let delta = incoming(0.0, 0.02);
        let (verdict, nudged) = evaluate_delta(&crystal, &delta);
        assert_eq!(verdict, SyncVerdict::Nudge);
        assert!(nudged.is_some());

        apply_nudge(&mut crystal, &delta, 0.5);
        let edge = crystal.edges.get("edge-local").unwrap();
        let last = edge.trajectory.last().unwrap();
        assert!((last.phase - 0.15).abs() < 1e-9);
    }

    #[test]
    fn scenario_reject() {
        let crystal = base_crystal(0.0, 0.02);
        let delta = incoming(PI, 0.02);
        let (verdict, nudged) = evaluate_delta(&crystal, &delta);
        assert_eq!(verdict, SyncVerdict::Reject);
        assert!(nudged.is_none());
    }
}
