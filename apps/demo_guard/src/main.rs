use csif_guard::evaluate_contradiction;
use rwif_core::Edge;
use std::f64::consts::PI;

fn main() {
    let baseline = Edge {
        edge_id: "e_baseline".into(),
        source_id: "light".into(),
        relation: "dispels".into(),
        target_id: "darkness".into(),
        theta: 0.0,
        sigma: 0.02,
    };

    let hallucination = Edge {
        edge_id: "e_halluc".into(),
        source_id: "darkness".into(),
        relation: "absorbs".into(),
        target_id: "light".into(),
        theta: PI,
        sigma: 0.02,
    };

    let decision = evaluate_contradiction(&baseline, &hallucination, 0.5);

    println!("CSIF-Guard (Rust) demo");
    println!("max residual: {:.4}", decision.max_residual);
    println!("threshold: {:.4}", decision.threshold);
    println!(
        "decision: {}",
        if decision.intercepted {
            "INTERCEPT"
        } else {
            "ALLOW"
        }
    );
}
