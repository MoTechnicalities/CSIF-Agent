//! Agent loop and orchestration logic for CSIF-Agent

use crate::grammar::{Grammar, QueryIntent, TeachFact};
use crate::metadata::{
    load_metadata, metadata_path_for_bank, migrate_schema_v1_to_v2, save_metadata, AgentMetadata,
    CURRENT_SCHEMA_VERSION,
};
use crate::relation::{RelationRegistry, RelationType};
use chrono::Utc;
use csif_cache::{CachedResponse, InvertedIndex, PreflightResult, QueryCache};
use csif_guard::PhaseGraph;
use csif_sync::{evaluate_delta, SyncDelta, SyncVerdict};
use rwif_core::{PhaseEvent, Provenance, RWIFCrystal, RWIFEdge, RWIFNode};
use std::collections::{HashSet, VecDeque};
use std::error::Error;
use std::path::{Path, PathBuf};

pub struct CSIFAgent {
    pub cache: QueryCache,
    pub index: InvertedIndex,
    pub crystal: RWIFCrystal,
    pub bank_path: PathBuf,
    pub grammar: Grammar,
    pub relation_registry: RelationRegistry,
    // Guard is built from crystal on demand
    // Sync is optional (for multi-agent setups)
}

impl CSIFAgent {
    /// Load existing crystal bank or create new one
    pub fn load_or_create(bank_path: &Path) -> Result<Self, Box<dyn Error>> {
        Self::load_or_create_with_grammar(bank_path, Path::new("./grammar.toml"))
    }

    /// Load existing crystal bank or create new one with explicit grammar config
    pub fn load_or_create_with_grammar(
        bank_path: &Path,
        grammar_path: &Path,
    ) -> Result<Self, Box<dyn Error>> {
        let bank_exists = bank_path.exists();
        let mut crystal = if bank_exists {
            RWIFCrystal::load_from_path(bank_path)?
        } else {
            RWIFCrystal {
                id: "my_brain".to_string(),
                nodes: Default::default(),
                edges: Default::default(),
            }
        };

        let grammar = if grammar_path.exists() {
            Grammar::load_from_path(grammar_path)?
        } else {
            Grammar::default()
        };

        let meta_path = metadata_path_for_bank(bank_path);
        let mut metadata = load_metadata(&meta_path)?.unwrap_or_else(|| AgentMetadata {
            schema_version: if bank_exists { 1 } else { CURRENT_SCHEMA_VERSION },
            grammar_version: if bank_exists {
                "legacy".to_string()
            } else {
                grammar.version().to_string()
            },
        });

        let mut metadata_changed = false;
        let mut crystal_changed = false;

        if metadata.schema_version < CURRENT_SCHEMA_VERSION {
            if metadata.schema_version == 1 {
                crystal_changed |= migrate_schema_v1_to_v2(&mut crystal);
                metadata.schema_version = CURRENT_SCHEMA_VERSION;
                metadata_changed = true;
            }
        }

        if metadata.grammar_version != grammar.version() {
            metadata.grammar_version = grammar.version().to_string();
            metadata_changed = true;
        }

        if crystal_changed {
            crystal.save_to_path(bank_path)?;
        }
        if metadata_changed {
            save_metadata(&meta_path, &metadata)?;
        }

        let mut index = InvertedIndex::new();
        index.index_crystal(&crystal);

        Ok(CSIFAgent {
            cache: QueryCache::new(),
            index,
            crystal,
            bank_path: bank_path.to_path_buf(),
            grammar,
            relation_registry: RelationRegistry::default(),
        })
    }

    /// Save crystal bank to disk
    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        self.crystal.save_to_path(&self.bank_path)?;
        Ok(())
    }

    /// Process a natural language query (or structured input)
    pub fn query(&mut self, input: &str) -> String {
        let Some(intent) = self.grammar.parse_query(input) else {
            return "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string();
        };

        if let Some(answer) = self.answer_from_crystal(&intent) {
            let query_phase = 0.0;
            let query_sigma = 0.02;
            let cache_key = cache_key_for_intent(&intent);
            let cached = CachedResponse {
                response: answer.clone(),
                resonance: 0.0,
                sigma: query_sigma,
                candidate_node_id: format!("n_{}", slug(subject_hint_for_intent(&intent))),
            };
            self.cache.insert(&cache_key, query_phase, query_sigma, cached);
            return format!("[CRYSTAL] {}", answer);
        }

        let cache_key = cache_key_for_intent(&intent);
        let query_phase = 0.0;
        let query_sigma = 0.02;

        match self.cache.preflight(
            &cache_key,
            query_phase,
            query_sigma,
            &self.crystal,
            &self.index,
            0.05,
            0.05,
        ) {
            PreflightResult::ShortCircuit(response) => format!("[CACHE] {}", response.response),
            PreflightResult::CacheMiss | PreflightResult::NeedsDeepValidation => {
                "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string()
            }
        }
    }

    pub fn teach(&mut self, input: &str) -> String {
        let Some(fact) = self.grammar.parse_teach(input) else {
            return "[NEEDS_INPUT] I can only learn factual statements from configured grammar patterns."
                .to_string();
        };

        if self.check_contradiction(&fact) {
            return "[CONTRADICTION] That contradicts what I already know.".to_string();
        }
        if !self.learn_from_fact(&fact) {
            return "[NEEDS_INPUT] I couldn't map that statement to a supported relation.".to_string();
        }
        self.save().unwrap_or_default();
        "[TEACHING] Knowledge crystallized.".to_string()
    }

    /// Answer a query directly from the crystal (without cache)
    fn answer_from_crystal(&self, intent: &QueryIntent) -> Option<String> {
        match intent {
            QueryIntent::Describe { subject } => {
                let mut is_a_targets = self.direct_targets_for_relation(subject, RelationType::IsA);
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
            QueryIntent::ConfirmRelation {
                relation,
                subject,
                object,
            } => {
                let Some(relation_type) = RelationType::from_str(relation) else {
                    return Some(format!(
                        "NO: relation '{}' is not registered for inference.",
                        relation
                    ));
                };

                if self.infer_relation(subject, object, relation_type) {
                    Some(format_relation_confirmation(subject, object, relation_type, true))
                } else {
                    Some(format_relation_confirmation(subject, object, relation_type, false))
                }
            }
            QueryIntent::ComputeAdd { left, right } => {
                let result = left + right;
                Some(format!(
                    "[COMPUTE] {} + {} = {}",
                    format_number(*left),
                    format_number(*right),
                    format_number(result)
                ))
            }
        }
    }

    /// Check if an input contradicts existing knowledge
    fn check_contradiction(&self, fact: &TeachFact) -> bool {
        let graph = PhaseGraph::from_crystal(&self.crystal);
        let _current_conflict = graph.max_multipath_conflict();

        let Some(relation_type) = RelationType::from_str(&fact.relation) else {
            return false;
        };

        if relation_type == RelationType::IsA {
            for edge in self.crystal.edges.values() {
                let Some(source_label) = node_label_by_id(&self.crystal, &edge.source) else {
                    continue;
                };
                let Some(target_label) = node_label_by_id(&self.crystal, &edge.target) else {
                    continue;
                };

                if source_label == fact.subject
                    && edge.relation == relation_type.as_str()
                    && target_label != fact.object
                {
                    return true;
                }
            }
        }

        false
    }

    /// Learn new knowledge from parsed fact
    fn learn_from_fact(&mut self, fact: &TeachFact) -> bool {
        let Some(relation_type) = RelationType::from_str(&fact.relation) else {
            return false;
        };

        let subject_id = ensure_node(&mut self.crystal, &fact.subject);
        let object_id = ensure_node(&mut self.crystal, &fact.object);
        let edge_id = format!(
            "e_{}_{}_{}",
            slug(&fact.subject),
            relation_type.as_str(),
            slug(&fact.object)
        );

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
                    relation: relation_type.as_str().to_string(),
                    target: object_id,
                    lobe: "English".to_string(),
                    trajectory: vec![event],
                },
            );
        }

        self.index.index_crystal(&self.crystal);
        true
    }

    fn infer_relation(&self, subject: &str, object: &str, relation: RelationType) -> bool {
        let Some(spec) = self.relation_registry.spec_by_type(relation) else {
            return false;
        };

        if !spec.transitive {
            return self
                .direct_targets_for_relation(subject, relation)
                .iter()
                .any(|target| target == object);
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([subject.to_string()]);

        while let Some(current) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }

            for next in self.direct_targets_for_relation(&current, relation) {
                if next == object {
                    return true;
                }
                if !visited.contains(&next) {
                    queue.push_back(next);
                }
            }
        }

        false
    }

    fn direct_targets_for_relation(&self, subject: &str, relation: RelationType) -> Vec<String> {
        let mut targets = Vec::new();
        for edge in self.crystal.edges.values() {
            if edge.relation != relation.as_str() {
                continue;
            }
            let Some(source_label) = node_label_by_id(&self.crystal, &edge.source) else {
                continue;
            };
            if source_label == subject {
                if let Some(target_label) = node_label_by_id(&self.crystal, &edge.target) {
                    targets.push(target_label.to_string());
                }
            }
        }
        targets
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

fn cache_key_for_intent(intent: &QueryIntent) -> String {
    match intent {
        QueryIntent::Describe { subject } => subject.clone(),
        QueryIntent::ConfirmRelation {
            relation,
            subject,
            object,
        } => format!("{}|{}|{}", subject, relation, object),
        QueryIntent::ComputeAdd { left, right } => {
            format!("compute|add|{}|{}", format_number(*left), format_number(*right))
        }
    }
}

fn subject_hint_for_intent(intent: &QueryIntent) -> &str {
    match intent {
        QueryIntent::Describe { subject } => subject,
        QueryIntent::ConfirmRelation { subject, .. } => subject,
        QueryIntent::ComputeAdd { .. } => "compute",
    }
}

fn format_relation_confirmation(
    subject: &str,
    object: &str,
    relation: RelationType,
    is_true: bool,
) -> String {
    match (relation, is_true) {
        (RelationType::IsA, true) => format!(
            "YES: {} {} is {} {}.",
            article_for(subject),
            subject,
            article_for(object),
            object
        ),
        (RelationType::IsA, false) => format!(
            "NO: I cannot establish that {} {} is {} {} from the crystal.",
            article_for(subject),
            subject,
            article_for(object),
            object
        ),
        (RelationType::Causes, true) => format!("YES: {} causes {}.", subject, object),
        (RelationType::Causes, false) => {
            format!("NO: I cannot establish that {} causes {}.", subject, object)
        }
        (RelationType::HasProperty, true) => {
            format!("YES: {} has {}.", subject, object)
        }
        (RelationType::HasProperty, false) => {
            format!("NO: I cannot establish that {} has {}.", subject, object)
        }
    }
}

fn format_number(n: f64) -> String {
    if (n.fract()).abs() < f64::EPSILON {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn slug(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
}

fn article_for(word: &str) -> &'static str {
    match word.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
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
    use crate::metadata::metadata_path_for_bank;
    use std::fs;

    const TEST_GRAMMAR: &str = r#"
version = "v2"

[query]
what_is = "^what is (?:a|an)?\\s*(.+?)\\?$"
is_a_confirm = "^is (?:a|an )?(.+?) (?:a|an) (.+?)\\?$"
causes_confirm = "^does (?:a|an )?(.+?) cause (.+?)\\?$"
has_property_confirm = "^does (?:a|an )?(.+?) have (.+?)\\?$"
add_compute = "^what is\\s+(-?\\d+(?:\\.\\d+)?)\\s*\\+\\s*(-?\\d+(?:\\.\\d+)?)\\?$"

[teach]
is_a = "^(?:a|an) (.+?) is (?:a|an) (.+)$"
causes = "^(.+?) causes (.+)$"
has_property = "^(?:a|an) (.+?) has (.+)$"
"#;

    fn temp_bank_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("csif_agent_{name}_{nanos}.json"))
    }

    fn temp_grammar_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("csif_agent_grammar_{name}_{nanos}.toml"));
        fs::write(&path, TEST_GRAMMAR).unwrap();
        path
    }

    #[test]
    fn is_a_relation_supports_transitive_inference() {
        let bank_path = temp_bank_path("is_a");
        let grammar_path = temp_grammar_path("is_a");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        assert_eq!(
            agent.teach("A whale is a mammal."),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("A mammal is an animal."),
            "[TEACHING] Knowledge crystallized."
        );

        let answer = agent.query("Is a whale an animal?");
        assert_eq!(answer, "[CRYSTAL] YES: a whale is an animal.");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn causes_relation_supports_transitive_inference() {
        let bank_path = temp_bank_path("causes");
        let grammar_path = temp_grammar_path("causes");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        assert_eq!(
            agent.teach("rain causes wet ground"),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("wet ground causes slippery"),
            "[TEACHING] Knowledge crystallized."
        );

        let answer = agent.query("Does rain cause slippery?");
        assert_eq!(answer, "[CRYSTAL] YES: rain causes slippery.");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn has_property_relation_is_direct_only() {
        let bank_path = temp_bank_path("has_property");
        let grammar_path = temp_grammar_path("has_property");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        assert_eq!(
            agent.teach("a whale has warm-blooded"),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("a mammal has vertebrate"),
            "[TEACHING] Knowledge crystallized."
        );

        assert_eq!(
            agent.query("Does a whale have warm-blooded?"),
            "[CRYSTAL] YES: whale has warm-blooded."
        );
        assert_eq!(
            agent.query("Does a whale have vertebrate?"),
            "[CRYSTAL] NO: I cannot establish that whale has vertebrate."
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn compute_add_scaffold_is_available() {
        let bank_path = temp_bank_path("compute");
        let grammar_path = temp_grammar_path("compute");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("What is 2 + 2?");
        assert_eq!(answer, "[CRYSTAL] [COMPUTE] 2 + 2 = 4");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn startup_migration_normalizes_legacy_article_nodes() {
        let bank_path = temp_bank_path("migration");
        let grammar_path = temp_grammar_path("migration");

        let mut legacy = RWIFCrystal {
            id: "legacy".to_string(),
            nodes: Default::default(),
            edges: Default::default(),
        };
        let whale = ensure_node(&mut legacy, "whale");
        let mammal = ensure_node(&mut legacy, "a mammal");
        let animal = ensure_node(&mut legacy, "an animal");

        let now = Utc::now();
        legacy.edges.insert(
            "e_old_1".to_string(),
            RWIFEdge {
                edge_id: "e_old_1".to_string(),
                source: whale.clone(),
                relation: "is_a".to_string(),
                target: mammal.clone(),
                lobe: "English".to_string(),
                trajectory: vec![PhaseEvent {
                    timestamp: now,
                    phase: 0.0,
                    sigma: 0.02,
                    source: Provenance {
                        source_type: "legacy".to_string(),
                        source_id: "seed".to_string(),
                    },
                }],
            },
        );
        legacy.edges.insert(
            "e_old_2".to_string(),
            RWIFEdge {
                edge_id: "e_old_2".to_string(),
                source: mammal,
                relation: "is_a".to_string(),
                target: animal,
                lobe: "English".to_string(),
                trajectory: vec![PhaseEvent {
                    timestamp: now,
                    phase: 0.0,
                    sigma: 0.02,
                    source: Provenance {
                        source_type: "legacy".to_string(),
                        source_id: "seed".to_string(),
                    },
                }],
            },
        );
        legacy.save_to_path(&bank_path).unwrap();

        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();
        let meta_path = metadata_path_for_bank(&bank_path);
        assert!(meta_path.exists());

        let answer = agent.query("Is a whale an animal?");
        assert_eq!(answer, "[CRYSTAL] YES: a whale is an animal.");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(meta_path);
        let _ = fs::remove_file(grammar_path);
    }
}
