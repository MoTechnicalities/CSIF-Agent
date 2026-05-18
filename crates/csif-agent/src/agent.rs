//! Agent loop and orchestration logic for CSIF-Agent

use csif_cache::{QueryCache, PreflightResult, InvertedIndex, CachedResponse};
use csif_guard::PhaseGraph;
use csif_sync::{SyncVerdict, SyncDelta, evaluate_delta};
use rwif_core::{RWIFCrystal, RWIFNode, RWIFEdge, PhaseEvent, Provenance};
use chrono::Utc;
use std::path::Path;
use std::error::Error;

pub struct CSIFAgent {
    pub cache: QueryCache,
    pub index: InvertedIndex,
    pub crystal: RWIFCrystal,
    // Guard is built from crystal on demand
    // Sync is optional (for multi-agent setups)
}

impl CSIFAgent {
    /// Load existing crystal bank or create new one
    pub fn load_or_create(bank_path: &Path) -> Result<Self, Box<dyn Error>> {
        let crystal = if bank_path.exists() {
            RWIFCrystal::load_from_path(bank_path)?
        } else {
            // No RWIFCrystal::new() exists; create a minimal empty crystal
            RWIFCrystal {
                id: "my_brain".to_string(),
                nodes: Default::default(),
                edges: Default::default(),
            }
        };
        let mut index = InvertedIndex::new();
        index.index_crystal(&crystal);
        Ok(CSIFAgent {
            cache: QueryCache::new(),
            index,
            crystal,
        })
    }

    /// Save crystal bank to disk
    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        self.crystal.save_to_path(Path::new("./my_brain.rwif"))?;
        Ok(())
    }

    /// Process a natural language query (or structured input)
    pub fn query(&mut self, input: &str) -> String {
        let query_phase = 0.0;
        let query_sigma = 0.02;
        let resonance_threshold = 0.05;
        let sigma_threshold = 0.05;
        match self.cache.preflight(
            input,
            query_phase,
            query_sigma,
            &self.crystal,
            &self.index,
            resonance_threshold,
            sigma_threshold,
        ) {
            PreflightResult::ShortCircuit(response) => {
                format!("[CACHE] {}", response.response)
            }
            PreflightResult::CacheMiss => {
                if let Some(answer) = self.answer_from_crystal(input) {
                    let cached = CachedResponse {
                        response: answer.clone(),
                        resonance: 0.0,
                        sigma: query_sigma,
                        candidate_node_id: "unknown".to_string(),
                    };
                    self.cache.insert(input, query_phase, query_sigma, cached);
                    format!("[CRYSTAL] {}", answer)
                } else {
                    "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string()
                }
            }
            PreflightResult::NeedsDeepValidation => {
                if self.check_contradiction(input) {
                    return "[CONTRADICTION] That contradicts what I already know.".to_string();
                }
                if self.learn_from_input(input) {
                    self.save().unwrap_or_default();
                    if let Some(answer) = self.answer_from_crystal(input) {
                        format!("[CRYSTAL] {}", answer)
                    } else {
                        "[LEARNED] I've incorporated that knowledge.".to_string()
                    }
                } else if let Some(answer) = self.answer_from_crystal(input) {
                    format!("[CRYSTAL] {}", answer)
                } else {
                    "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string()
                }
            }
        }
    }

    pub fn teach(&mut self, input: &str) -> String {
        if self.check_contradiction(input) {
            return "[CONTRADICTION] That contradicts what I already know.".to_string();
        }
        if !self.learn_from_input(input) {
            return "[NEEDS_INPUT] I can only learn factual statements like 'A whale is a mammal.'".to_string();
        }
        self.save().unwrap_or_default();
        "[TEACHING] Knowledge crystallized.".to_string()
    }

    /// Answer a query directly from the crystal (without cache)
    fn answer_from_crystal(&self, input: &str) -> Option<String> {
        let subject = parse_query_subject(input)?;
        let mut is_a_targets = Vec::new();
        for edge in self.crystal.edges.values() {
            let Some(source_label) = node_label_by_id(&self.crystal, &edge.source) else {
                continue;
            };
            if source_label == subject && edge.relation == "is_a" {
                if let Some(target_label) = node_label_by_id(&self.crystal, &edge.target) {
                    is_a_targets.push(target_label.to_string());
                }
            }
        }
        if !is_a_targets.is_empty() {
            is_a_targets.sort();
            is_a_targets.dedup();
            return Some(format!("A {} is {}.", subject, is_a_targets.join(" and ")));
        }
        None
    }

    /// Check if an input contradicts existing knowledge
    fn check_contradiction(&self, input: &str) -> bool {
        let Some((subject, target)) = parse_fact_is_a(input) else {
            return false;
        };

        // Existing graph sanity check (kept for visibility and future extensions).
        let graph = PhaseGraph::from_crystal(&self.crystal);
        let _current_conflict = graph.max_multipath_conflict();

        // If we already know a different class for the same subject, reject.
        for edge in self.crystal.edges.values() {
            let Some(source_label) = node_label_by_id(&self.crystal, &edge.source) else {
                continue;
            };
            let Some(target_label) = node_label_by_id(&self.crystal, &edge.target) else {
                continue;
            };

            if source_label == subject && edge.relation == "is_a" && target_label != target {
                return true;
            }
        }

        false
    }

    /// Learn new knowledge from input
    fn learn_from_input(&mut self, input: &str) -> bool {
        let Some((subject, target)) = parse_fact_is_a(input) else {
            return false;
        };

        let subject_id = ensure_node(&mut self.crystal, &subject);
        let target_id = ensure_node(&mut self.crystal, &target);
        let edge_id = format!("e_{}_is_a_{}", slug(&subject), slug(&target));

        let event = PhaseEvent {
            timestamp: Utc::now(),
            phase: 0.0,
            sigma: 0.02,
            source: Provenance {
                source_type: "teach".to_string(),
                source_id: "local".to_string(),
            },
        };

        if let Some(edge) = self.crystal.edges.get_mut(&edge_id) {
            edge.trajectory.push(event);
        } else {
            self.crystal.edges.insert(
                edge_id.clone(),
                RWIFEdge {
                    edge_id,
                    source: subject_id,
                    relation: "is_a".to_string(),
                    target: target_id,
                    lobe: "English".to_string(),
                    trajectory: vec![event],
                },
            );
        }

        self.index.index_crystal(&self.crystal);
        true
    }

    /// Sync with another agent (for multi-agent setups)
    pub fn sync_receive(&mut self, delta: &SyncDelta) -> SyncVerdict {
        let (verdict, _nudge) = evaluate_delta(&self.crystal, delta);
        verdict
    }

    pub fn sync_broadcast(&self) -> Option<SyncDelta> {
        // TODO: Generate delta from recent changes
        None
    }
}

fn normalize_text(text: &str) -> String {
    text.trim().trim_end_matches('.').trim().to_lowercase()
}

fn slug(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
}

fn parse_fact_is_a(input: &str) -> Option<(String, String)> {
    let normalized = normalize_text(input);
    let rest = if let Some(r) = normalized.strip_prefix("a ") {
        r
    } else if let Some(r) = normalized.strip_prefix("an ") {
        r
    } else {
        return None;
    };

    let mut parts = rest.splitn(2, " is ");
    let subject = parts.next()?.trim();
    let target = parts.next()?.trim();
    if subject.is_empty() || target.is_empty() {
        return None;
    }
    Some((subject.to_string(), target.to_string()))
}

fn parse_query_subject(input: &str) -> Option<String> {
    let normalized = input.trim().to_lowercase();
    if let Some(rest) = normalized.strip_prefix("what is ") {
        return Some(rest.trim().trim_end_matches('?').to_string());
    }
    None
}

fn ensure_node(crystal: &mut RWIFCrystal, label: &str) -> String {
    if let Some((id, _)) = crystal.nodes.iter().find(|(_, n)| n.label == label) {
        return id.clone();
    }

    let node_id = format!("n_{}", slug(label));
    crystal.nodes.insert(
        node_id.clone(),
        RWIFNode {
            node_id: node_id.clone(),
            label: label.to_string(),
        },
    );
    node_id
}

fn node_label_by_id<'a>(crystal: &'a RWIFCrystal, node_id: &str) -> Option<&'a str> {
    crystal.nodes.get(node_id).map(|n| n.label.as_str())
}
