//! Agent loop and orchestration logic for CSIF-Agent

use crate::grammar::{Grammar, QueryIntent, TeachFact};
use crate::metadata::{
    load_lobe_state, load_metadata, lobe_state_path_for_bank, metadata_path_for_bank,
    migrate_schema_v1_to_v2, save_lobe_state, save_metadata, AgentMetadata, AppliedLobe,
    CURRENT_SCHEMA_VERSION,
};
use crate::relation::{RelationRegistry, RelationType};
use chrono::Utc;
use csif_cache::{CachedResponse, InvertedIndex, PreflightResult, QueryCache};
use csif_sync::{evaluate_delta, SyncDelta, SyncVerdict};
use rwif_core::{PhaseEvent, Provenance, RWIFCrystal, RWIFEdge, RWIFNode};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub struct CSIFAgent {
    pub cache: QueryCache,
    pub index: InvertedIndex,
    pub crystal: RWIFCrystal,
    pub bank_path: PathBuf,
    pub grammar: Grammar,
    pub relation_registry: RelationRegistry,
    index_dirty: bool,
    save_every: usize,
    pending_saves: usize,
    // Guard is built from crystal on demand
    // Sync is optional (for multi-agent setups)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LobeRefreshReport {
    pub discovered: usize,
    pub applied: usize,
    pub skipped: usize,
    pub taught: usize,
    pub ignored: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct LobeManifest {
    id: String,
    version: String,
    seed_files: Vec<String>,
    compatible_agent: Option<String>,
    priority: Option<i32>,
    enabled: Option<bool>,
    checksum_sha256: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
struct LobeBundle {
    bundle_dir: PathBuf,
    manifest_path: PathBuf,
    manifest: LobeManifest,
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

        let save_every = std::env::var("CSIF_SAVE_EVERY")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(1);

        let mut agent = CSIFAgent {
            cache: QueryCache::new(),
            index,
            crystal,
            bank_path: bank_path.to_path_buf(),
            grammar,
            relation_registry: RelationRegistry::default(),
            index_dirty: false,
            save_every,
            pending_saves: 0,
        };

        if std::env::var("CSIF_LOBES_DIR").is_ok() {
            let report = agent.refresh_lobes_from_env()?;
            eprintln!(
                "Lobe refresh: discovered={} applied={} skipped={} ignored={} taught={}",
                report.discovered, report.applied, report.skipped, report.ignored, report.taught
            );
        }

        Ok(agent)
    }

    /// Save crystal bank to disk
    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        self.crystal.save_to_path(&self.bank_path)?;
        Ok(())
    }

    /// Process a natural language query (or structured input)
    pub fn query(&mut self, input: &str) -> String {
        if self.index_dirty {
            self.index.index_crystal(&self.crystal);
            self.index_dirty = false;
        }

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

        if matches!(intent, QueryIntent::Describe { .. }) {
            return "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string();
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

        self.pending_saves = self.pending_saves.saturating_add(1);
        if self.pending_saves >= self.save_every {
            self.save().unwrap_or_default();
            self.pending_saves = 0;
        }

        "[TEACHING] Knowledge crystallized.".to_string()
    }

    /// Fast path for curated seed facts: parse + ingest without contradiction gate.
    pub fn ingest_seed_fact(&mut self, input: &str) -> bool {
        let Some(fact) = self.grammar.parse_teach(input) else {
            return false;
        };

        if !self.learn_from_fact(&fact) {
            return false;
        }

        self.pending_saves = self.pending_saves.saturating_add(1);
        if self.pending_saves >= self.save_every {
            self.save().ok();
            self.pending_saves = 0;
        }

        true
    }

    /// Flush pending batched work to disk/index.
    pub fn flush(&mut self) -> Result<(), Box<dyn Error>> {
        if self.index_dirty {
            self.index.index_crystal(&self.crystal);
            self.index_dirty = false;
        }
        if self.pending_saves > 0 {
            self.save()?;
            self.pending_saves = 0;
        }
        Ok(())
    }

    /// Load and apply lobe bundles from CSIF_LOBES_DIR, if configured.
    pub fn refresh_lobes_from_env(&mut self) -> Result<LobeRefreshReport, Box<dyn Error>> {
        let lobe_dir = match std::env::var("CSIF_LOBES_DIR") {
            Ok(path) if !path.trim().is_empty() => PathBuf::from(path),
            _ => return Ok(LobeRefreshReport::default()),
        };
        self.refresh_lobes_from_dir(&lobe_dir)
    }

    /// Return the persisted list of applied lobe bundles for the current bank.
    pub fn applied_lobes(&self) -> Result<Vec<AppliedLobe>, Box<dyn Error>> {
        let state_path = lobe_state_path_for_bank(&self.bank_path);
        let state = load_lobe_state(&state_path)?;
        Ok(state.applied)
    }

    /// Load and apply lobe bundles from an explicit directory.
    pub fn refresh_lobes_from_dir(
        &mut self,
        lobe_dir: &Path,
    ) -> Result<LobeRefreshReport, Box<dyn Error>> {
        if !lobe_dir.exists() {
            return Ok(LobeRefreshReport::default());
        }

        let strict = std::env::var("CSIF_LOBES_STRICT")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

        let bundles = collect_lobe_bundles(lobe_dir)?;
        let mut report = LobeRefreshReport {
            discovered: bundles.len(),
            ..LobeRefreshReport::default()
        };

        let state_path = lobe_state_path_for_bank(&self.bank_path);
        let mut lobe_state = load_lobe_state(&state_path)?;
        let mut state_changed = false;

        let mut sorted_bundles = bundles;
        sorted_bundles.sort_by(|a, b| {
            let ap = a.manifest.priority.unwrap_or(100);
            let bp = b.manifest.priority.unwrap_or(100);
            ap.cmp(&bp)
                .then(a.manifest.id.cmp(&b.manifest.id))
                .then(a.manifest.version.cmp(&b.manifest.version))
        });

        let agent_version = Version::parse(env!("CARGO_PKG_VERSION"))?;

        for bundle in sorted_bundles {
            match self.apply_lobe_bundle(&bundle, &agent_version, &mut lobe_state) {
                Ok((applied, taught, ignored)) => {
                    if applied {
                        state_changed = true;
                        report.applied += 1;
                    } else {
                        report.skipped += 1;
                    }
                    report.taught += taught;
                    report.ignored += ignored;
                }
                Err(err) => {
                    if strict {
                        return Err(err);
                    }
                    eprintln!(
                        "Skipping lobe bundle {} due to error: {}",
                        bundle.manifest_path.display(),
                        err
                    );
                    report.ignored += 1;
                }
            }
        }

        if state_changed {
            save_lobe_state(&state_path, &lobe_state)?;
        }

        Ok(report)
    }

    fn apply_lobe_bundle(
        &mut self,
        bundle: &LobeBundle,
        agent_version: &Version,
        lobe_state: &mut crate::metadata::LobeState,
    ) -> Result<(bool, usize, usize), Box<dyn Error>> {
        if bundle.manifest.enabled == Some(false) {
            return Ok((false, 0, 1));
        }

        if let Some(req) = &bundle.manifest.compatible_agent {
            let req = VersionReq::parse(req)?;
            if !req.matches(agent_version) {
                return Ok((false, 0, 1));
            }
        }

        let fingerprint = lobe_fingerprint(bundle)?;
        let already_applied = lobe_state.applied.iter().any(|entry| {
            entry.id == bundle.manifest.id
                && entry.version == bundle.manifest.version
                && entry.fingerprint == fingerprint
        });
        if already_applied {
            return Ok((false, 0, 0));
        }

        let mut taught = 0usize;
        let mut ignored = 0usize;
        for rel_seed in &bundle.manifest.seed_files {
            let seed_path = bundle.bundle_dir.join(rel_seed);
            if !seed_path.exists() {
                return Err(format!("missing seed file: {}", seed_path.display()).into());
            }

            if let Some(expected_hex) = bundle
                .manifest
                .checksum_sha256
                .as_ref()
                .and_then(|m| m.get(rel_seed))
            {
                let actual_hex = file_sha256_hex(&seed_path)?;
                if actual_hex.to_lowercase() != expected_hex.to_lowercase() {
                    return Err(format!(
                        "checksum mismatch for {} (expected {}, got {})",
                        seed_path.display(),
                        expected_hex,
                        actual_hex
                    )
                    .into());
                }
            }

            let file = fs::File::open(&seed_path)?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let raw = line?;
                let fact = raw.trim();
                if fact.is_empty() {
                    continue;
                }
                if self.ingest_seed_fact(fact) {
                    taught += 1;
                } else {
                    ignored += 1;
                }
            }
        }

        self.flush()?;
        lobe_state.applied.push(AppliedLobe {
            id: bundle.manifest.id.clone(),
            version: bundle.manifest.version.clone(),
            fingerprint,
        });

        Ok((true, taught, ignored))
    }

    /// Answer a query directly from the crystal (without cache)
    fn answer_from_crystal(&self, intent: &QueryIntent) -> Option<String> {
        match intent {
            QueryIntent::Describe { subject } => {
                let mut is_a_targets = self.direct_targets_for_relation(subject, RelationType::IsA);
                if !is_a_targets.is_empty() {
                    is_a_targets.sort();
                    is_a_targets.dedup();
                    let rendered_target_items = is_a_targets
                        .iter()
                        .map(|target| format!("{} {}", article_for(target), target))
                        .collect::<Vec<_>>();
                    let rendered_targets = natural_language_list_with_connector(
                        &rendered_target_items,
                        "and",
                        true,
                    );

                    let properties = self.direct_targets_for_relation(subject, RelationType::HasProperty);
                    let subtypes = self.subtypes_for(subject, RelationType::IsA);
                    let templates = self.grammar.describe_templates();
                    let mut response = render_describe_classification(
                        &templates.classification,
                        subject,
                        &rendered_targets,
                    );

                    if !properties.is_empty() {
                        let rendered_properties = properties
                            .iter()
                            .map(|property| property.replace('_', " "))
                            .collect::<Vec<_>>();
                        let property_list = natural_language_list_with_connector(
                            &rendered_properties,
                            &templates.property_connector,
                            templates.oxford_comma,
                        );
                        append_describe_clause(
                            &mut response,
                            &templates.properties_intro,
                            &property_list,
                            &templates.properties_outro,
                        );
                    }

                    if !subtypes.is_empty() && templates.max_subtype_examples > 0 {
                        let rendered_subtypes = subtypes
                            .iter()
                            .take(templates.max_subtype_examples)
                            .map(|subtype| subtype.replace('_', " "))
                            .collect::<Vec<_>>();
                        let subtype_list = natural_language_list_with_connector(
                            &rendered_subtypes,
                            &templates.subtype_connector,
                            templates.oxford_comma,
                        );
                        append_describe_clause(
                            &mut response,
                            &templates.subtypes_intro,
                            &subtype_list,
                            &templates.subtypes_outro,
                        );
                    }

                    return Some(response);
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
            QueryIntent::ComputeArithmetic {
                left,
                operator,
                right,
            } => {
                let result = match operator.as_str() {
                    "+" => decimal_add(left, right),
                    "-" => decimal_sub(left, right),
                    "*" => decimal_mul(left, right),
                    "/" => decimal_div(left, right, 18),
                    _ => None,
                };

                if let Some(result) = result {
                    Some(format!("[COMPUTE] {} {} {} = {}", left, operator, right, result))
                } else if operator == "/" {
                    Some("[COMPUTE] division is undefined for the provided values".to_string())
                } else {
                    Some("[COMPUTE] unable to compute exact decimal result".to_string())
                }
            }
        }
    }

    /// Check if an input contradicts existing knowledge
    fn check_contradiction(&self, fact: &TeachFact) -> bool {
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

        self.index_dirty = true;
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

    fn subtypes_for(&self, parent: &str, relation: RelationType) -> Vec<String> {
        let mut subtypes = Vec::new();
        for edge in self.crystal.edges.values() {
            if edge.relation != relation.as_str() {
                continue;
            }

            let Some(target_label) = node_label_by_id(&self.crystal, &edge.target) else {
                continue;
            };
            if target_label != parent {
                continue;
            }

            if let Some(source_label) = node_label_by_id(&self.crystal, &edge.source) {
                subtypes.push(source_label.to_string());
            }
        }

        subtypes.sort();
        subtypes.dedup();
        subtypes
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
        QueryIntent::ComputeArithmetic {
            left,
            operator,
            right,
        } => format!("compute|{}|{}|{}", operator, left, right),
    }
}

fn subject_hint_for_intent(intent: &QueryIntent) -> &str {
    match intent {
        QueryIntent::Describe { subject } => subject,
        QueryIntent::ConfirmRelation { subject, .. } => subject,
        QueryIntent::ComputeArithmetic { .. } => "compute",
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
            format!(
                "YES: {} {} is {}.",
                article_for(subject),
                subject,
                object
            )
        }
        (RelationType::HasProperty, false) => {
            format!(
                "NO: I cannot establish that {} {} is {}.",
                article_for(subject),
                subject,
                object
            )
        }
    }
}

fn decimal_add(left: &str, right: &str) -> Option<String> {
    let (lv, ls) = parse_decimal_scaled(left)?;
    let (rv, rs) = parse_decimal_scaled(right)?;
    let scale = ls.max(rs);

    let lmul = pow10_i128(scale.checked_sub(ls)?)?;
    let rmul = pow10_i128(scale.checked_sub(rs)?)?;
    let lnorm = lv.checked_mul(lmul)?;
    let rnorm = rv.checked_mul(rmul)?;
    let sum = lnorm.checked_add(rnorm)?;

    Some(format_decimal_scaled(sum, scale))
}

fn decimal_sub(left: &str, right: &str) -> Option<String> {
    let (lv, ls) = parse_decimal_scaled(left)?;
    let (rv, rs) = parse_decimal_scaled(right)?;
    let scale = ls.max(rs);

    let lmul = pow10_i128(scale.checked_sub(ls)?)?;
    let rmul = pow10_i128(scale.checked_sub(rs)?)?;
    let lnorm = lv.checked_mul(lmul)?;
    let rnorm = rv.checked_mul(rmul)?;
    let diff = lnorm.checked_sub(rnorm)?;

    Some(format_decimal_scaled(diff, scale))
}

fn decimal_mul(left: &str, right: &str) -> Option<String> {
    let (lv, ls) = parse_decimal_scaled(left)?;
    let (rv, rs) = parse_decimal_scaled(right)?;
    let scale = ls.checked_add(rs)?;
    let product = lv.checked_mul(rv)?;

    Some(format_decimal_scaled(product, scale))
}

fn decimal_div(left: &str, right: &str, max_precision: u32) -> Option<String> {
    let (lv, ls) = parse_decimal_scaled(left)?;
    let (rv, rs) = parse_decimal_scaled(right)?;
    if rv == 0 {
        return None;
    }

    let numerator = lv.checked_mul(pow10_i128(rs)?)?;
    let denominator = rv.checked_mul(pow10_i128(ls)?)?;
    if denominator == 0 {
        return None;
    }

    let negative = (numerator < 0) ^ (denominator < 0);
    let n_abs = numerator.checked_abs()?;
    let d_abs = denominator.checked_abs()?;

    let int_part = n_abs / d_abs;
    let mut remainder = n_abs % d_abs;
    let mut fractional = String::new();

    for _ in 0..max_precision {
        if remainder == 0 {
            break;
        }
        remainder = remainder.checked_mul(10)?;
        let digit = remainder / d_abs;
        remainder %= d_abs;
        fractional.push(char::from(b'0' + u8::try_from(digit).ok()?));
    }

    while fractional.ends_with('0') {
        fractional.pop();
    }

    let mut out = if fractional.is_empty() {
        int_part.to_string()
    } else {
        format!("{}.{}", int_part, fractional)
    };

    if out != "0" && negative {
        out.insert(0, '-');
    }

    Some(out)
}

fn parse_decimal_scaled(input: &str) -> Option<(i128, u32)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (sign, body) = if let Some(rest) = trimmed.strip_prefix('-') {
        (-1i128, rest)
    } else if let Some(rest) = trimmed.strip_prefix('+') {
        (1i128, rest)
    } else {
        (1i128, trimmed)
    };

    let mut parts = body.split('.');
    let int_part = parts.next()?;
    let frac_part = parts.next();
    if parts.next().is_some() {
        return None;
    }

    if int_part.is_empty() || !int_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let frac = frac_part.unwrap_or("");
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let scale = frac.len() as u32;
    let digits = if frac.is_empty() {
        int_part.to_string()
    } else {
        format!("{}{}", int_part, frac)
    };

    let mut value = digits.parse::<i128>().ok()?;
    value = value.checked_mul(sign)?;
    Some((value, scale))
}

fn format_decimal_scaled(value: i128, scale: u32) -> String {
    if scale == 0 {
        return value.to_string();
    }

    let negative = value < 0;
    let mut digits = value.abs().to_string();
    let scale_usize = scale as usize;

    if digits.len() <= scale_usize {
        let pad = "0".repeat(scale_usize + 1 - digits.len());
        digits = format!("{}{}", pad, digits);
    }

    let split = digits.len() - scale_usize;
    let int_part = &digits[..split];
    let frac_part = &digits[split..];
    let frac_trimmed = frac_part.trim_end_matches('0');

    let mut out = if frac_trimmed.is_empty() {
        int_part.to_string()
    } else {
        format!("{}.{}", int_part, frac_trimmed)
    };

    if out == "0" {
        return out;
    }

    if negative {
        out.insert(0, '-');
    }
    out
}

fn pow10_i128(exp: u32) -> Option<i128> {
    let mut value = 1i128;
    for _ in 0..exp {
        value = value.checked_mul(10)?;
    }
    Some(value)
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

fn natural_language_list_with_connector(
    items: &[String],
    connector: &str,
    oxford_comma: bool,
) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} {} {}", items[0], connector, items[1]),
        _ => {
            let head = items[..items.len() - 1].join(", ");
            if oxford_comma {
                format!("{}, {} {}", head, connector, items[items.len() - 1])
            } else {
                format!("{} {} {}", head, connector, items[items.len() - 1])
            }
        }
    }
}

fn render_describe_classification(template: &str, subject: &str, direct: &str) -> String {
    template
        .replace("{subject}", subject)
        .replace("{direct}", direct)
}

fn append_describe_clause(response: &mut String, intro: &str, content: &str, outro: &str) {
    if intro.trim().is_empty() || content.trim().is_empty() {
        return;
    }

    if !response.ends_with(' ') {
        response.push(' ');
    }
    response.push_str(intro.trim());
    response.push(' ');
    response.push_str(content);
    response.push_str(outro);
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

fn collect_lobe_bundles(lobe_dir: &Path) -> Result<Vec<LobeBundle>, Box<dyn Error>> {
    let mut manifest_paths = Vec::<PathBuf>::new();

    let mut roots = fs::read_dir(lobe_dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    roots.sort_by_key(|entry| entry.path());

    for root in roots {
        let root_path = root.path();
        let direct_manifest = root_path.join("lobe.toml");
        if direct_manifest.exists() {
            manifest_paths.push(direct_manifest);
        }

        let mut nested = fs::read_dir(&root_path)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .collect::<Vec<_>>();
        nested.sort_by_key(|entry| entry.path());
        for child in nested {
            let nested_manifest = child.path().join("lobe.toml");
            if nested_manifest.exists() {
                manifest_paths.push(nested_manifest);
            }
        }
    }

    manifest_paths.sort();
    manifest_paths.dedup();

    let mut bundles = Vec::new();
    for manifest_path in manifest_paths {
        let raw = fs::read_to_string(&manifest_path)?;
        let manifest: LobeManifest = toml::from_str(&raw)?;
        if manifest.id.trim().is_empty() {
            return Err(format!("invalid lobe id in {}", manifest_path.display()).into());
        }
        if manifest.version.trim().is_empty() {
            return Err(format!("invalid lobe version in {}", manifest_path.display()).into());
        }
        if manifest.seed_files.is_empty() {
            return Err(format!("no seed_files in {}", manifest_path.display()).into());
        }

        bundles.push(LobeBundle {
            bundle_dir: manifest_path
                .parent()
                .ok_or("manifest has no parent directory")?
                .to_path_buf(),
            manifest_path,
            manifest,
        });
    }

    Ok(bundles)
}

fn lobe_fingerprint(bundle: &LobeBundle) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();

    let manifest_raw = fs::read(&bundle.manifest_path)?;
    hasher.update(b"manifest:");
    hasher.update(&manifest_raw);

    let mut seed_files = bundle.manifest.seed_files.clone();
    seed_files.sort();
    for rel in seed_files {
        let seed_path = bundle.bundle_dir.join(&rel);
        let raw = fs::read(&seed_path)?;
        hasher.update(b"seed:");
        hasher.update(rel.as_bytes());
        hasher.update(b"\n");
        hasher.update(raw);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn file_sha256_hex(path: &Path) -> Result<String, Box<dyn Error>> {
    let raw = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(raw);
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::metadata_path_for_bank;
    use std::fs;

    const TEST_GRAMMAR: &str = r#"
version = "v2"

[query]
what_is = "^what is (?:(?:a|an)\\s+)?(.+?)\\?$"
is_a_confirm = "^is (?:(?:a|an)\\s+)?(.+?) (?:a|an) (.+?)\\?$"
causes_confirm = "^does (?:(?:a|an)\\s+)?(.+?) cause (.+?)\\?$"
has_property_confirm = "^does (?:(?:a|an)\\s+)?(.+?) have (.+?)\\?$"
add_compute = "^what is\\s+(-?\\d+(?:\\.\\d+)?)\\s*([+\\-*/])\\s*(-?\\d+(?:\\.\\d+)?)\\?$"

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
            "[CRYSTAL] YES: a whale is warm-blooded."
        );
        assert_eq!(
            agent.query("Does a whale have vertebrate?"),
            "[CRYSTAL] NO: I cannot establish that a whale is vertebrate."
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

        let subtract = agent.query("What is 10 - 3?");
        assert_eq!(subtract, "[CRYSTAL] [COMPUTE] 10 - 3 = 7");

        let multiply = agent.query("What is 2.5 * 4?");
        assert_eq!(multiply, "[CRYSTAL] [COMPUTE] 2.5 * 4 = 10");

        let divide = agent.query("What is 7 / 2?");
        assert_eq!(divide, "[CRYSTAL] [COMPUTE] 7 / 2 = 3.5");

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
