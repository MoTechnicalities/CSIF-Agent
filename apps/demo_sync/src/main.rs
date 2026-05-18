use csif_sync::{apply_nudge, consensus_report, evaluate_delta, EdgeUpdate, SyncDelta, SyncVerdict};
use rwif_core::{PhaseEvent, Provenance, RWIFCrystal, RWIFEdge, RWIFNode};
use std::collections::HashMap;
use std::f64::consts::PI;

fn make_crystal(local_phase: f64, local_sigma: f64) -> RWIFCrystal {
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

    let mut edges = HashMap::new();
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
                    source_id: "demo".to_string(),
                },
            }],
        },
    );

    RWIFCrystal {
        id: "crystal-sync-demo".to_string(),
        nodes,
        edges,
    }
}

fn incoming_delta(phase: f64, sigma: f64) -> SyncDelta {
    SyncDelta {
        source_agent: "agent-a".to_string(),
        crystal_id: "crystal-sync-demo".to_string(),
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

fn main() {
    println!("CSIF-Sync (Rust) demo");

    let c_skip = make_crystal(0.0, 0.02);
    let d_skip = incoming_delta(0.0, 0.02);
    let (v_skip, _) = evaluate_delta(&c_skip, &d_skip);
    println!("scenario SKIP verdict: {:?}", v_skip);
    assert_eq!(v_skip, SyncVerdict::Skip);

    let mut c_nudge = make_crystal(0.30, 0.10);
    let d_nudge = incoming_delta(0.0, 0.02);
    let (v_nudge, nphase) = evaluate_delta(&c_nudge, &d_nudge);
    println!("scenario NUDGE verdict: {:?}, nudged_phase: {:?}", v_nudge, nphase);
    assert_eq!(v_nudge, SyncVerdict::Nudge);
    apply_nudge(&mut c_nudge, &d_nudge, 0.5);

    let c_reject = make_crystal(0.0, 0.02);
    let d_reject = incoming_delta(PI, 0.02);
    let (v_reject, _) = evaluate_delta(&c_reject, &d_reject);
    println!("scenario REJECT verdict: {:?}", v_reject);
    assert_eq!(v_reject, SyncVerdict::Reject);

    let agent_phases = vec![0.05, 0.06, 0.04, 0.07, 0.05];

    let report = consensus_report(&agent_phases, 0.05).expect("phases should not be empty");

    println!("consensus phase: {:.4}", report.consensus_phase);
    println!("max deviation: {:.4}", report.max_deviation);
    println!("converged: {}", report.converged);
}
