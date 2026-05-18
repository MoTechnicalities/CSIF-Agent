use csif_core::{normalized_resonance, temporal_wave_phase};
use rwif_core::RWIFCrystal;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct CachedResponse {
    pub response: String,
    pub resonance: f64,
    pub sigma: f64,
    pub candidate_node_id: String,
}

#[derive(Clone, Debug)]
pub enum PreflightResult {
    ShortCircuit(CachedResponse),
    CacheMiss,
    NeedsDeepValidation,
}

#[derive(Clone, Debug)]
pub struct InvertedIndex {
    pub token_to_node: HashMap<u64, Vec<String>>,
    node_labels: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct QueryCache {
    pub query_to_response: HashMap<String, CachedResponse>,
}

impl InvertedIndex {
    pub fn new() -> Self {
        Self {
            token_to_node: HashMap::new(),
            node_labels: HashMap::new(),
        }
    }

    pub fn index_crystal(&mut self, crystal: &RWIFCrystal) {
        self.token_to_node.clear();
        self.node_labels.clear();

        for (node_id, node) in &crystal.nodes {
            self.node_labels.insert(node_id.clone(), node.label.clone());
            for token in normalize_tokens(&node.label) {
                let slot = token_slot64(&token);
                self.token_to_node
                    .entry(slot)
                    .or_default()
                    .push(node_id.clone());
            }
        }

        for ids in self.token_to_node.values_mut() {
            ids.sort();
            ids.dedup();
        }
    }

    fn candidate_scores(&self, query: &str) -> HashMap<String, usize> {
        let mut score: HashMap<String, usize> = HashMap::new();
        for token in normalize_tokens(query) {
            let slot = token_slot64(&token);
            if let Some(node_ids) = self.token_to_node.get(&slot) {
                for node_id in node_ids {
                    *score.entry(node_id.clone()).or_insert(0) += 1;
                }
            }
        }
        score
    }
}

impl Default for InvertedIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryCache {
    pub fn new() -> Self {
        Self {
            query_to_response: HashMap::new(),
        }
    }

    pub fn preflight(
        &self,
        query: &str,
        query_phase: f64,
        query_sigma: f64,
        crystal: &RWIFCrystal,
        index: &InvertedIndex,
        resonance_threshold: f64,
        sigma_threshold: f64,
    ) -> PreflightResult {
        let key = query_key(query, query_phase, query_sigma);
        if let Some(hit) = self.query_to_response.get(&key) {
            return PreflightResult::ShortCircuit(hit.clone());
        }

        let scores = index.candidate_scores(query);
        if scores.is_empty() {
            return PreflightResult::CacheMiss;
        }

        let mut ranked: Vec<(String, usize)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let candidate_node = &ranked[0].0;

        let (memory_phase, memory_sigma) = node_state(crystal, candidate_node);
        let resonance = normalized_resonance(query_phase, memory_phase);
        let sigma = query_sigma.max(memory_sigma);

        if memory_sigma <= sigma_threshold && resonance < resonance_threshold {
            let response = best_fact_for_node(crystal, candidate_node)
                .unwrap_or_else(|| index.node_labels.get(candidate_node).cloned().unwrap_or_default());
            return PreflightResult::ShortCircuit(CachedResponse {
                response,
                resonance,
                sigma: memory_sigma,
                candidate_node_id: candidate_node.clone(),
            });
        }

        let _ = sigma;
        PreflightResult::NeedsDeepValidation
    }

    pub fn insert(&mut self, query: &str, query_phase: f64, query_sigma: f64, response: CachedResponse) {
        let key = query_key(query, query_phase, query_sigma);
        self.query_to_response.insert(key, response);
    }
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum RouteOutcome {
    PreflightShortCircuit,
    CacheMiss,
    DeepValidation,
}

#[derive(Debug)]
pub struct RouteDecision {
    pub resonance: f64,
    pub evolved_phase: f64,
    pub outcome: RouteOutcome,
}

pub fn route_query(
    query_phase: f64,
    memory_phase: f64,
    sigma: f64,
    t: f64,
    resonance_threshold: f64,
    sigma_threshold: f64,
) -> RouteDecision {
    let evolved = temporal_wave_phase(memory_phase, sigma, t);
    let r = normalized_resonance(query_phase, evolved);

    let outcome = if r < resonance_threshold && sigma <= sigma_threshold {
        RouteOutcome::PreflightShortCircuit
    } else {
        RouteOutcome::DeepValidation
    };

    RouteDecision {
        resonance: r,
        evolved_phase: evolved,
        outcome,
    }
}

fn normalize_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            out.push(current.clone());
            current.clear();
        }
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}

fn token_slot64(token: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[0..8]);
    u64::from_be_bytes(bytes)
}

fn query_key(query: &str, query_phase: f64, query_sigma: f64) -> String {
    let canonical = format!("{}|{:.6}|{:.6}", query.trim().to_lowercase(), query_phase, query_sigma);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn node_state(crystal: &RWIFCrystal, node_id: &str) -> (f64, f64) {
    let mut samples = Vec::new();

    for edge in crystal.edges.values() {
        if edge.source == node_id || edge.target == node_id {
            if let Some(last) = edge.trajectory.last() {
                samples.push((last.phase, last.sigma));
            }
        }
    }

    if samples.is_empty() {
        return (0.0, 1.0);
    }

    let phase = samples.iter().map(|(p, _)| *p).sum::<f64>() / samples.len() as f64;
    let sigma = samples.iter().map(|(_, s)| *s).fold(0.0_f64, f64::max);
    (phase, sigma)
}

fn best_fact_for_node(crystal: &RWIFCrystal, node_id: &str) -> Option<String> {
    for edge in crystal.edges.values() {
        if edge.source == node_id {
            let source = crystal.nodes.get(&edge.source)?.label.clone();
            let target = crystal.nodes.get(&edge.target)?.label.clone();
            return Some(format!("{} {} {}.", source, edge.relation, target));
        }
    }

    crystal.nodes.get(node_id).map(|n| n.label.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rwif_core::{PhaseEvent, Provenance, RWIFEdge, RWIFNode};

    fn crystal(memory_sigma: f64) -> RWIFCrystal {
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
                    sigma: memory_sigma,
                    source: Provenance {
                        source_type: "seed".to_string(),
                        source_id: "test".to_string(),
                    },
                }],
            },
        );

        RWIFCrystal {
            id: "c1".to_string(),
            nodes,
            edges,
        }
    }

    #[test]
    fn preflight_short_circuit_and_cache_hit() {
        let crystal = crystal(0.02);
        let mut index = InvertedIndex::new();
        index.index_crystal(&crystal);

        let mut cache = QueryCache::new();
        let q = "light";

        let first = cache.preflight(q, 0.0, 0.02, &crystal, &index, 0.05, 0.05);
        match first {
            PreflightResult::ShortCircuit(resp) => {
                cache.insert(q, 0.0, 0.02, resp.clone());
            }
            _ => panic!("expected short circuit"),
        }

        let second = cache.preflight(q, 0.0, 0.02, &crystal, &index, 0.05, 0.05);
        match second {
            PreflightResult::ShortCircuit(resp) => {
                assert_eq!(resp.candidate_node_id, "n_light");
            }
            _ => panic!("expected cache hit via short circuit"),
        }
    }

    #[test]
    fn preflight_cache_miss() {
        let crystal = crystal(0.02);
        let mut index = InvertedIndex::new();
        index.index_crystal(&crystal);
        let cache = QueryCache::new();

        let result = cache.preflight("whale", 0.0, 0.02, &crystal, &index, 0.05, 0.05);
        assert!(matches!(result, PreflightResult::CacheMiss));
    }

    #[test]
    fn preflight_needs_deep_validation() {
        let crystal = crystal(0.20);
        let mut index = InvertedIndex::new();
        index.index_crystal(&crystal);
        let cache = QueryCache::new();

        let result = cache.preflight("light", 0.0, 0.02, &crystal, &index, 0.05, 0.05);
        assert!(matches!(result, PreflightResult::NeedsDeepValidation));
    }
}
