//! Agent loop and orchestration logic for CSIF-Agent

use csif_cache::{QueryCache, PreflightResult, InvertedIndex, CachedResponse};
use csif_guard::PhaseGraph;
use csif_sync::{SyncVerdict, SyncDelta, evaluate_delta};
use rwif_core::{RWIFCrystal, RWIFNode, RWIFEdge, PhaseEvent, Provenance};
use chrono::Utc;
use std::path::Path;
use std::path::PathBuf;
use std::error::Error;
use std::collections::{HashSet, VecDeque};

pub struct CSIFAgent {
    pub cache: QueryCache,
    pub index: InvertedIndex,
    pub crystal: RWIFCrystal,
    pub bank_path: PathBuf,
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
            bank_path: bank_path.to_path_buf(),
        })
    }

    /// Save crystal bank to disk
    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        self.crystal.save_to_path(&self.bank_path)?;
        Ok(())
    }

    /// Process a natural language query (or structured input)
    pub fn query(&mut self, input: &str) -> String {
        let Some(query) = parse_query(input) else {
            return "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string();
        };

        if let Some(answer) = self.answer_from_crystal(&query) {
            let query_phase = 0.0;
            let query_sigma = 0.02;
            let cache_key = query.cache_key();
            let cached = CachedResponse {
                response: answer.clone(),
                resonance: 0.0,
                sigma: query_sigma,
                candidate_node_id: format!("n_{}", slug(query.subject())),
            };
            self.cache.insert(&cache_key, query_phase, query_sigma, cached);
            return format!("[CRYSTAL] {}", answer);
        }

        let cache_key = query.cache_key();

        let query_phase = 0.0;
        let query_sigma = 0.02;
        let resonance_threshold = 0.05;
        let sigma_threshold = 0.05;

        match self.cache.preflight(
            &cache_key,
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
                "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string()
            }
            PreflightResult::NeedsDeepValidation => {
                "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string()
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
    fn answer_from_crystal(&self, query: &Query) -> Option<String> {
        match query {
            Query::Describe { subject } => {
                let mut is_a_targets = self.direct_is_a_targets(subject);
                if !is_a_targets.is_empty() {
                    is_a_targets.sort();
                    is_a_targets.dedup();
                    let rendered_targets = is_a_targets
                        .iter()
                        .map(|target| format!("{} {}", article_for(target), target))
                        .collect::<Vec<_>>()
                        .join(" and ");
                    return Some(format!("A {} is {}.", subject, rendered_targets));
                }
                None
            }
            Query::IsA { subject, target } => {
                if self.has_transitive_is_a(subject, target) {
                    Some(format!(
                        "YES: {} {} is {} {}.",
                        article_for(subject),
                        subject,
                        article_for(target),
                        target
                    ))
                } else {
                    Some(format!(
                        "NO: I cannot establish that {} {} is {} {} from the crystal.",
                        article_for(subject),
                        subject,
                        article_for(target),
                        target
                    ))
                }
            }
        }
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

enum Query {
    Describe { subject: String },
    IsA { subject: String, target: String },
}

impl Query {
    fn subject(&self) -> &str {
        match self {
            Query::Describe { subject } => subject,
            Query::IsA { subject, .. } => subject,
        }
    }

    fn cache_key(&self) -> String {
        match self {
            Query::Describe { subject } => subject.clone(),
            Query::IsA { subject, target } => format!("{}|is_a|{}", subject, target),
        }
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
    let mut target = parts.next()?.trim().to_string();
    if let Some(stripped) = target.strip_prefix("a ") {
        target = stripped.trim().to_string();
    } else if let Some(stripped) = target.strip_prefix("an ") {
        target = stripped.trim().to_string();
    }
    if subject.is_empty() || target.is_empty() {
        return None;
    }
    Some((subject.to_string(), target))
}

fn parse_query_subject(input: &str) -> Option<String> {
    let normalized = input.trim().to_lowercase();
    if let Some(rest) = normalized.strip_prefix("what is ") {
        let mut subject = rest.trim().trim_end_matches('?').to_string();
        // Strip leading articles ("a " or "an ") from subject
        if subject.starts_with("a ") {
            subject = subject[2..].to_string();
        } else if subject.starts_with("an ") {
            subject = subject[3..].to_string();
        }
        return if subject.is_empty() { None } else { Some(subject) };
    }
    None
}

fn parse_is_a_query(input: &str) -> Option<(String, String)> {
    let normalized = input.trim().trim_end_matches('?').to_lowercase();
    let mut rest = normalized.strip_prefix("is ")?.trim();

    if let Some(stripped) = rest.strip_prefix("a ") {
        rest = stripped.trim();
    } else if let Some(stripped) = rest.strip_prefix("an ") {
        rest = stripped.trim();
    }

    let (subject, target) = if let Some((subject, target)) = rest.split_once(" an ") {
        (subject.trim(), target.trim())
    } else if let Some((subject, target)) = rest.split_once(" a ") {
        (subject.trim(), target.trim())
    } else {
        return None;
    };

    if subject.is_empty() || target.is_empty() {
        return None;
    }

    Some((subject.to_string(), target.to_string()))
}

fn parse_query(input: &str) -> Option<Query> {
    if let Some((subject, target)) = parse_is_a_query(input) {
        return Some(Query::IsA { subject, target });
    }

    parse_query_subject(input).map(|subject| Query::Describe { subject })
}

fn article_for(word: &str) -> &'static str {
    match word.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

impl CSIFAgent {
    fn direct_is_a_targets(&self, subject: &str) -> Vec<String> {
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
        is_a_targets
    }

    fn has_transitive_is_a(&self, subject: &str, target: &str) -> bool {
        if subject == target {
            return true;
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([subject.to_string()]);

        while let Some(current) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }

            for next in self.direct_is_a_targets(&current) {
                if next == target {
                    return true;
                }
                if !visited.contains(&next) {
                    queue.push_back(next);
                }
            }
        }

        false
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_bank_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("csif_agent_{name}_{nanos}.json"))
    }

    #[test]
    fn describe_query_returns_direct_is_a_fact() {
        let bank_path = temp_bank_path("describe");
        let mut agent = CSIFAgent::load_or_create(&bank_path).unwrap();

        assert_eq!(agent.teach("A whale is a mammal."), "[TEACHING] Knowledge crystallized.");

        let answer = agent.query("What is a whale?");
        assert_eq!(answer, "[CRYSTAL] A whale is a mammal.");

        let _ = std::fs::remove_file(bank_path);
    }

    #[test]
    fn is_a_query_uses_transitive_inference() {
        let bank_path = temp_bank_path("transitive");
        let mut agent = CSIFAgent::load_or_create(&bank_path).unwrap();

        assert_eq!(agent.teach("A whale is a mammal."), "[TEACHING] Knowledge crystallized.");
        assert_eq!(agent.teach("A mammal is an animal."), "[TEACHING] Knowledge crystallized.");

        let answer = agent.query("Is a whale an animal?");
        assert_eq!(answer, "[CRYSTAL] YES: a whale is an animal.");

        let _ = std::fs::remove_file(bank_path);
    }
}
