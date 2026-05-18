use csif_cache::{InvertedIndex, PreflightResult, QueryCache};
use rwif_core::{PhaseEvent, Provenance, RWIFCrystal, RWIFEdge, RWIFNode};
use std::collections::HashMap;

fn build_crystal(sigma: f64) -> RWIFCrystal {
    let mut nodes = HashMap::new();
    nodes.insert(
        "n_light".to_string(),
        RWIFNode {
            node_id: "n_light".to_string(),
            label: "light".to_string(),
        },
    );
    nodes.insert(
        "n_dark".to_string(),
        RWIFNode {
            node_id: "n_dark".to_string(),
            label: "darkness".to_string(),
        },
    );

    let mut edges = HashMap::new();
    edges.insert(
        "e1".to_string(),
        RWIFEdge {
            edge_id: "e1".to_string(),
            source: "n_light".to_string(),
            relation: "dispels".to_string(),
            target: "n_dark".to_string(),
            lobe: "English".to_string(),
            trajectory: vec![PhaseEvent {
                timestamp: "2026-05-18T00:00:00Z".parse().unwrap(),
                phase: 0.0,
                sigma,
                source: Provenance {
                    source_type: "seed".to_string(),
                    source_id: "demo".to_string(),
                },
            }],
        },
    );

    RWIFCrystal {
        id: "demo-cache".to_string(),
        nodes,
        edges,
    }
}

fn main() {
    println!("CSIF-Cache (Rust) demo");

    let crystal_low_sigma = build_crystal(0.02);
    let mut index = InvertedIndex::new();
    index.index_crystal(&crystal_low_sigma);
    let mut cache = QueryCache::new();

    let q = "light";
    let first = cache.preflight(q, 0.0, 0.02, &crystal_low_sigma, &index, 0.05, 0.05);
    match first {
        PreflightResult::ShortCircuit(resp) => {
            println!("scenario 1: PREFLIGHT_SHORT_CIRCUIT ({})", resp.response);
            cache.insert(q, 0.0, 0.02, resp);
        }
        _ => panic!("expected short circuit"),
    }

    let second = cache.preflight(q, 0.0, 0.02, &crystal_low_sigma, &index, 0.05, 0.05);
    match second {
        PreflightResult::ShortCircuit(_) => println!("scenario 2: CACHE_HIT (via deterministic query cache)"),
        _ => panic!("expected cache hit"),
    }

    let crystal_high_sigma = build_crystal(0.20);
    let mut index_high = InvertedIndex::new();
    index_high.index_crystal(&crystal_high_sigma);
    let deep = QueryCache::new().preflight("light", 0.0, 0.02, &crystal_high_sigma, &index_high, 0.05, 0.05);
    match deep {
        PreflightResult::NeedsDeepValidation => println!("scenario 3: DEEP_VALIDATION"),
        _ => panic!("expected deep validation"),
    }

    let miss = QueryCache::new().preflight("whale", 0.0, 0.02, &crystal_low_sigma, &index, 0.05, 0.05);
    match miss {
        PreflightResult::CacheMiss => println!("scenario 4: CACHE_MISS"),
        _ => panic!("expected cache miss"),
    }
}
