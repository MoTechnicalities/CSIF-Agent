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
            QueryIntent::ComputeExpression { expression } => {
                Some(render_compute_expression(expression))
            }
            QueryIntent::SolveEquation { equation } => Some(render_solve_equation(equation)),
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
        QueryIntent::ComputeExpression { expression } => {
            format!("compute|{}", expression)
        }
        QueryIntent::SolveEquation { equation } => format!("solve|{}", equation),
    }
}

fn subject_hint_for_intent(intent: &QueryIntent) -> &str {
    match intent {
        QueryIntent::Describe { subject } => subject,
        QueryIntent::ConfirmRelation { subject, .. } => subject,
        QueryIntent::ComputeExpression { .. } => "compute",
        QueryIntent::SolveEquation { .. } => "solve",
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

fn render_compute_expression(expression: &str) -> String {
    let parsed = parse_math_expression(expression)
        .and_then(|node| node.evaluate().map(|value| (node, value)));

    match parsed {
        Some((node, value)) => {
            let rendered = format_compute_value(value);
            let mut out = format!("[COMPUTE] {} = {}", expression, rendered);
            if compute_latex_enabled() {
                out.push_str(&format!("\n$$ {} = {} $$", node.to_latex(), rendered));
            }
            out
        }
        None => "[COMPUTE] unable to evaluate expression".to_string(),
    }
}

fn render_solve_equation(equation: &str) -> String {
    match solve_equation(equation) {
        EquationSolution::LinearUnique(x) => {
            let value = format_compute_value(x);
            let mut out = format!("[SOLVE] x = {}", value);
            if compute_latex_enabled() {
                out.push_str(&format!(
                    "\n$$ {} $$\n$$ x = {} $$",
                    equation_to_latex(equation),
                    value
                ));
            }
            out
        }
        EquationSolution::QuadraticTwoRoots(x1, x2) => {
            let left = format_compute_value(x1);
            let right = format_compute_value(x2);
            let mut out = format!("[SOLVE] x1 = {}, x2 = {}", left, right);
            if compute_latex_enabled() {
                out.push_str(&format!(
                    "\n$$ {} $$\n$$ x_1 = {}, x_2 = {} $$",
                    equation_to_latex(equation),
                    left,
                    right
                ));
            }
            out
        }
        EquationSolution::QuadraticOneRoot(x) => {
            let value = format_compute_value(x);
            let mut out = format!("[SOLVE] x = {}", value);
            if compute_latex_enabled() {
                out.push_str(&format!(
                    "\n$$ {} $$\n$$ x = {} $$",
                    equation_to_latex(equation),
                    value
                ));
            }
            out
        }
        EquationSolution::InfiniteSolutions => {
            "[SOLVE] infinitely many solutions".to_string()
        }
        EquationSolution::NoSolution => "[SOLVE] no solution".to_string(),
        EquationSolution::NoRealRoots => "[SOLVE] no real roots".to_string(),
        EquationSolution::Unsupported => {
            "[SOLVE] unsupported equation form; use linear or quadratic in x".to_string()
        }
    }
}

#[derive(Debug, Clone)]
enum EquationSolution {
    LinearUnique(f64),
    QuadraticTwoRoots(f64, f64),
    QuadraticOneRoot(f64),
    InfiniteSolutions,
    NoSolution,
    NoRealRoots,
    Unsupported,
}

fn solve_equation(equation: &str) -> EquationSolution {
    let compact = equation.replace(' ', "");
    let mut parts = compact.split('=');
    let Some(left) = parts.next() else {
        return EquationSolution::Unsupported;
    };
    let Some(right) = parts.next() else {
        return EquationSolution::Unsupported;
    };
    if parts.next().is_some() {
        return EquationSolution::Unsupported;
    }

    let Some((a1, b1, c1)) = parse_polynomial_up_to_quadratic(left) else {
        return EquationSolution::Unsupported;
    };
    let Some((a2, b2, c2)) = parse_polynomial_up_to_quadratic(right) else {
        return EquationSolution::Unsupported;
    };

    let a = a1 - a2;
    let b = b1 - b2;
    let c = c1 - c2;
    let eps = 1e-12;

    if a.abs() < eps {
        if b.abs() < eps {
            if c.abs() < eps {
                return EquationSolution::InfiniteSolutions;
            }
            return EquationSolution::NoSolution;
        }
        return EquationSolution::LinearUnique(-c / b);
    }

    let disc = b * b - 4.0 * a * c;
    if disc < -eps {
        return EquationSolution::NoRealRoots;
    }
    if disc.abs() <= eps {
        return EquationSolution::QuadraticOneRoot(-b / (2.0 * a));
    }

    let sqrt_disc = disc.sqrt();
    let r1 = (-b - sqrt_disc) / (2.0 * a);
    let r2 = (-b + sqrt_disc) / (2.0 * a);
    if r1 <= r2 {
        EquationSolution::QuadraticTwoRoots(r1, r2)
    } else {
        EquationSolution::QuadraticTwoRoots(r2, r1)
    }
}

fn parse_polynomial_up_to_quadratic(expr: &str) -> Option<(f64, f64, f64)> {
    if expr.is_empty() {
        return None;
    }

    let normalized = expr.replace('-', "+-");
    let normalized = if let Some(rest) = normalized.strip_prefix("+-") {
        rest.to_string()
    } else {
        normalized
    };

    let mut a = 0.0f64;
    let mut b = 0.0f64;
    let mut c = 0.0f64;

    for raw_term in normalized.split('+') {
        let term = raw_term.trim();
        if term.is_empty() {
            continue;
        }

        if let Some(coeff_raw) = term.strip_suffix("x^2") {
            let coeff = parse_symbolic_coeff(coeff_raw)?;
            a += coeff;
            continue;
        }

        if let Some(coeff_raw) = term.strip_suffix('x') {
            if term.contains('^') {
                return None;
            }
            let coeff = parse_symbolic_coeff(coeff_raw)?;
            b += coeff;
            continue;
        }

        c += term.parse::<f64>().ok()?;
    }

    Some((a, b, c))
}

fn parse_symbolic_coeff(text: &str) -> Option<f64> {
    match text {
        "" | "+" => Some(1.0),
        "-" => Some(-1.0),
        _ => text.parse::<f64>().ok(),
    }
}

fn equation_to_latex(equation: &str) -> String {
    equation
        .replace("x^2", "x^{2}")
        .replace('*', " \\cdot ")
}

fn compute_latex_enabled() -> bool {
    std::env::var("CSIF_COMPUTE_LATEX")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn format_compute_value(value: f64) -> String {
    if !value.is_finite() {
        return "undefined".to_string();
    }
    let normalized = if value.abs() < 1e-15 { 0.0 } else { value };
    let mut out = format!("{normalized:.15}");
    while out.contains('.') && out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    if out.is_empty() {
        "0".to_string()
    } else {
        out
    }
}

#[derive(Debug, Clone)]
enum MathNode {
    Number(f64),
    UnaryMinus(Box<MathNode>),
    Binary {
        op: char,
        left: Box<MathNode>,
        right: Box<MathNode>,
    },
    Function {
        name: String,
        arg: Box<MathNode>,
    },
}

impl MathNode {
    fn evaluate(&self) -> Option<f64> {
        match self {
            MathNode::Number(v) => Some(*v),
            MathNode::UnaryMinus(inner) => inner.evaluate().map(|v| -v),
            MathNode::Binary { op, left, right } => {
                let l = left.evaluate()?;
                let r = right.evaluate()?;
                match op {
                    '+' => Some(l + r),
                    '-' => Some(l - r),
                    '*' => Some(l * r),
                    '/' => {
                        if r == 0.0 {
                            None
                        } else {
                            Some(l / r)
                        }
                    }
                    '^' => Some(l.powf(r)),
                    _ => None,
                }
            }
            MathNode::Function { name, arg } => {
                let v = arg.evaluate()?;
                match name.as_str() {
                    "sqrt" => {
                        if v < 0.0 {
                            None
                        } else {
                            Some(v.sqrt())
                        }
                    }
                    "abs" => Some(v.abs()),
                    "sin" => Some(v.sin()),
                    "cos" => Some(v.cos()),
                    "tan" => Some(v.tan()),
                    "ln" => {
                        if v <= 0.0 {
                            None
                        } else {
                            Some(v.ln())
                        }
                    }
                    "log" => {
                        if v <= 0.0 {
                            None
                        } else {
                            Some(v.log10())
                        }
                    }
                    _ => None,
                }
            }
        }
    }

    fn precedence(&self) -> u8 {
        match self {
            MathNode::Number(_) => 5,
            MathNode::Function { .. } => 5,
            MathNode::UnaryMinus(_) => 4,
            MathNode::Binary { op, .. } => match op {
                '+' | '-' => 1,
                '*' | '/' => 2,
                '^' => 3,
                _ => 0,
            },
        }
    }

    fn to_latex(&self) -> String {
        self.to_latex_prec(0)
    }

    fn to_latex_prec(&self, parent_prec: u8) -> String {
        let rendered = match self {
            MathNode::Number(v) => format_compute_value(*v),
            MathNode::UnaryMinus(inner) => format!("-{}", inner.to_latex_prec(self.precedence())),
            MathNode::Binary { op, left, right } => match op {
                '+' => format!(
                    "{} + {}",
                    left.to_latex_prec(self.precedence()),
                    right.to_latex_prec(self.precedence())
                ),
                '-' => format!(
                    "{} - {}",
                    left.to_latex_prec(self.precedence()),
                    right.to_latex_prec(self.precedence() + 1)
                ),
                '*' => format!(
                    "{} \\cdot {}",
                    left.to_latex_prec(self.precedence()),
                    right.to_latex_prec(self.precedence())
                ),
                '/' => format!(
                    "\\frac{{{}}}{{{}}}",
                    left.to_latex_prec(0),
                    right.to_latex_prec(0)
                ),
                '^' => format!(
                    "{}^{{{}}}",
                    left.to_latex_prec(self.precedence()),
                    right.to_latex_prec(self.precedence())
                ),
                _ => String::new(),
            },
            MathNode::Function { name, arg } => match name.as_str() {
                "sqrt" => format!("\\sqrt{{{}}}", arg.to_latex_prec(0)),
                _ => format!("\\{}\\left({}\\right)", name, arg.to_latex_prec(0)),
            },
        };

        if self.precedence() < parent_prec {
            format!("\\left({}\\right)", rendered)
        } else {
            rendered
        }
    }
}

#[derive(Debug, Clone)]
enum MathToken {
    Number(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
}

fn parse_math_expression(expression: &str) -> Option<MathNode> {
    let tokens = tokenize_math(expression)?;
    let mut parser = MathParser { tokens, pos: 0 };
    let node = parser.parse_expression()?;
    if parser.is_end() { Some(node) } else { None }
}

fn tokenize_math(expression: &str) -> Option<Vec<MathToken>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = expression.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c.is_ascii_digit() || c == '.' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let raw: String = chars[start..i].iter().collect();
            tokens.push(MathToken::Number(raw.parse::<f64>().ok()?));
            continue;
        }

        if c.is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            tokens.push(MathToken::Ident(ident.to_lowercase()));
            continue;
        }

        match c {
            '+' | '-' | '*' | '/' | '^' => tokens.push(MathToken::Op(c)),
            '(' => tokens.push(MathToken::LParen),
            ')' => tokens.push(MathToken::RParen),
            _ => return None,
        }
        i += 1;
    }

    Some(tokens)
}

struct MathParser {
    tokens: Vec<MathToken>,
    pos: usize,
}

impl MathParser {
    fn is_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&MathToken> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<MathToken> {
        if self.is_end() {
            return None;
        }
        let token = self.tokens[self.pos].clone();
        self.pos += 1;
        Some(token)
    }

    fn parse_expression(&mut self) -> Option<MathNode> {
        let mut node = self.parse_term()?;
        loop {
            match self.peek() {
                Some(MathToken::Op('+')) => {
                    self.advance();
                    let right = self.parse_term()?;
                    node = MathNode::Binary {
                        op: '+',
                        left: Box::new(node),
                        right: Box::new(right),
                    };
                }
                Some(MathToken::Op('-')) => {
                    self.advance();
                    let right = self.parse_term()?;
                    node = MathNode::Binary {
                        op: '-',
                        left: Box::new(node),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Some(node)
    }

    fn parse_term(&mut self) -> Option<MathNode> {
        let mut node = self.parse_power()?;
        loop {
            match self.peek() {
                Some(MathToken::Op('*')) => {
                    self.advance();
                    let right = self.parse_power()?;
                    node = MathNode::Binary {
                        op: '*',
                        left: Box::new(node),
                        right: Box::new(right),
                    };
                }
                Some(MathToken::Op('/')) => {
                    self.advance();
                    let right = self.parse_power()?;
                    node = MathNode::Binary {
                        op: '/',
                        left: Box::new(node),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Some(node)
    }

    fn parse_power(&mut self) -> Option<MathNode> {
        let node = self.parse_unary()?;
        if matches!(self.peek(), Some(MathToken::Op('^'))) {
            self.advance();
            let right = self.parse_power()?;
            Some(MathNode::Binary {
                op: '^',
                left: Box::new(node),
                right: Box::new(right),
            })
        } else {
            Some(node)
        }
    }

    fn parse_unary(&mut self) -> Option<MathNode> {
        if matches!(self.peek(), Some(MathToken::Op('-'))) {
            self.advance();
            let inner = self.parse_unary()?;
            Some(MathNode::UnaryMinus(Box::new(inner)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Option<MathNode> {
        match self.advance()? {
            MathToken::Number(v) => Some(MathNode::Number(v)),
            MathToken::LParen => {
                let node = self.parse_expression()?;
                match self.advance()? {
                    MathToken::RParen => Some(node),
                    _ => None,
                }
            }
            MathToken::Ident(name) => {
                match self.advance()? {
                    MathToken::LParen => {
                        let arg = self.parse_expression()?;
                        match self.advance()? {
                            MathToken::RParen => Some(MathNode::Function {
                                name,
                                arg: Box::new(arg),
                            }),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
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
    fn compute_latex_respects_parentheses_precedence() {
        let node = parse_math_expression("(9 + 4) * 2").unwrap();
        assert_eq!(node.to_latex(), "\\left(9 + 4\\right) \\cdot 2");
    }

    #[test]
    fn compute_renderer_includes_parenthesized_latex() {
        std::env::set_var("CSIF_COMPUTE_LATEX", "1");
        let rendered = render_compute_expression("(9 + 4) * 2");
        std::env::remove_var("CSIF_COMPUTE_LATEX");
        assert!(rendered.contains("\\left(9 + 4\\right) \\cdot 2"));
    }

    #[test]
    fn solve_linear_equation_mode_returns_symbolic_solution() {
        let bank_path = temp_bank_path("solve_linear");
        let grammar_path = temp_grammar_path("solve_linear");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve 2x + 3 = 7");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x = 2");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_quadratic_equation_mode_returns_two_roots() {
        let bank_path = temp_bank_path("solve_quadratic");
        let grammar_path = temp_grammar_path("solve_quadratic");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve x^2 - 5x + 6 = 0");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x1 = 2, x2 = 3");

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
