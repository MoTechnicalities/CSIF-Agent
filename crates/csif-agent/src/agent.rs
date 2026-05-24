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
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::f64::consts::PI;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Instant;

thread_local! {
    static COMPUTE_LATEX_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

fn play_trace_slow_ms() -> Option<u64> {
    std::env::var("CSIF_PLAY_TRACE_SLOW_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
}

fn log_play_phase_if_slow(phase: &str, started_at: Instant, detail: impl FnOnce() -> String) {
    let Some(threshold_ms) = play_trace_slow_ms() else {
        return;
    };

    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    if elapsed_ms < threshold_ms {
        return;
    }

    eprintln!(
        "play-trace phase={phase} elapsed_ms={elapsed_ms} {}",
        detail()
    );
}

pub struct CSIFAgent {
    pub cache: QueryCache,
    pub index: InvertedIndex,
    pub crystal: RWIFCrystal,
    pub anti_lobe: RWIFCrystal,
    pub bank_path: PathBuf,
    pub anti_lobe_bank_path: PathBuf,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResultPayload {
    pub answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate: Option<ProofCertificate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clauses: Option<Vec<QueryClauseResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composite: Option<CompositeQuerySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_time_context: Option<RequestTimeContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_audit: Option<RouteAuditTrail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryClauseResult {
    pub input: String,
    pub answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate: Option<ProofCertificate>,
    pub semantic_projection: SemanticProjection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_audit: Option<RouteAuditTrail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeQuerySummary {
    pub clause_count: usize,
    pub clauses_with_certificates: usize,
    pub verified_certificates: usize,
    pub all_clause_certificates_verified: bool,
    pub intents: Vec<String>,
    pub meaning_vocabulary: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticProjection {
    pub intent: String,
    pub meaning_tokens: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainResultPayload {
    pub input: String,
    pub answer: String,
    pub intent: String,
    pub relation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_limit: Option<usize>,
    pub path: Vec<String>,
    pub considered_contradictions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_time_context: Option<RequestTimeContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_audit: Option<RouteAuditTrail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestTimeContext {
    pub request_received_at: String,
    pub unix_ms: i64,
    pub timezone: String,
    pub initiator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteAuditTrail {
    pub relation: Option<String>,
    pub subject: Option<String>,
    pub object: Option<String>,
    pub tried: Vec<String>,
    pub stop_reason: String,
    pub negative_evidence: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anti_lobe_bank_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlayAttemptOutcome {
    SuccessCrystallized,
    FailurePersisted,
    SkippedKnownFailure,
    SkippedKnownSuccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayAttempt {
    pub relation: String,
    pub subject: String,
    pub object: String,
    pub basis: Vec<String>,
    pub outcome: PlayAttemptOutcome,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiLobeEntry {
    pub relation: String,
    pub subject: String,
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_phase: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_source_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_source_id: Option<String>,
    pub trajectory_len: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiLobeSnapshot {
    pub bank_path: String,
    pub entry_count: usize,
    pub entries: Vec<AntiLobeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "domain", content = "payload", rename_all = "kebab-case")]
pub enum ProofCertificate {
    Math(SolveCertificate),
    Language(LanguageCertificate),
}

impl ProofCertificate {
    pub fn family(&self) -> &str {
        match self {
            ProofCertificate::Math(certificate) => certificate.family(),
            ProofCertificate::Language(certificate) => certificate.family(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageCertificate {
    family: String,
    modality: String,
    parse: LanguageParseCertificate,
    replay: LanguageCertificateReplay,
}

impl LanguageCertificate {
    pub fn family(&self) -> &str {
        &self.family
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LanguageParseCertificate {
    normalized_input: String,
    best: SemanticFormV1,
    alternatives: Vec<SemanticFormV1>,
    rejections: Vec<ParseRejection>,
    ambiguity_remaining: bool,
    clarification_trigger: Option<ClarificationTrigger>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SemanticFormV1 {
    version: u8,
    intent: SemanticIntent,
    atoms: Vec<SemanticAtom>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ParseRejection {
    label: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClarificationTrigger {
    question: String,
    options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SemanticAtom {
    primitive: SemanticPrimitive,
    role: Option<String>,
    value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum SemanticPrimitive {
    Entity,
    Class,
    Relation,
    Property,
    Assertion,
    Query,
    Causality,
    Descriptor,
    EvidencePath,
    Modality,
    State,
    Category,
    Event,
    Negation,
    Action,
    Instruction,
    Time,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum SemanticIntent {
    ConfirmRelation,
    DescribeEntity,
    InstructionRequest,
    NarrativeState,
    NarrativeEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum LanguageCertificateReplay {
    ConfirmRelation {
        relation: String,
        subject: String,
        object: String,
        proved: bool,
        witness_path: Vec<String>,
    },
    DescribeEntity {
        subject: String,
        direct_classes: Vec<String>,
        properties: Vec<String>,
        subtypes: Vec<String>,
    },
    InstructionRequest {
        action: String,
        target: Option<String>,
        negated: bool,
        ambiguous: bool,
        plan_steps: Vec<String>,
        safe_actions: Vec<SafeExecutableAction>,
        execution_policy: ActionExecutionPolicy,
        dry_run_results: Vec<SimulatedActionResult>,
    },
    NarrativeState {
        subject: String,
        state: String,
        negated: bool,
    },
    NarrativeEvent {
        actor: String,
        event: String,
        object: Option<String>,
        negated: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SafeExecutableAction {
    kind: String,
    command: String,
    rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ActionExecutionPolicy {
    mode: String,
    allow_mutation: bool,
    require_explicit_target: bool,
    approved_kinds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SimulatedActionResult {
    kind: String,
    command: String,
    allowed: bool,
    reason: String,
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

#[derive(Debug, Clone)]
struct RelationInferenceTrace {
    tried: Vec<String>,
    stop_reason: String,
}

fn current_request_time_context() -> RequestTimeContext {
    current_request_time_context_with_initiator("user")
}

pub fn current_request_time_context_with_initiator(initiator: &str) -> RequestTimeContext {
    let now = Utc::now();
    RequestTimeContext {
        request_received_at: now.to_rfc3339(),
        unix_ms: now.timestamp_millis(),
        timezone: "UTC".to_string(),
        initiator: initiator.to_string(),
    }
}

fn anti_lobe_bank_path(bank_path: &Path) -> PathBuf {
    let mut path = bank_path.to_path_buf();
    let stem = bank_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("csif_agent_bank");
    let ext = bank_path.extension().and_then(|value| value.to_str()).unwrap_or("json");
    path.set_file_name(format!("{stem}.anti.{ext}"));
    path
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
        let anti_lobe_bank_path = anti_lobe_bank_path(bank_path);
        let mut crystal = if bank_exists {
            RWIFCrystal::load_from_path(bank_path)?
        } else {
            RWIFCrystal {
                id: "my_brain".to_string(),
                nodes: Default::default(),
                edges: Default::default(),
            }
        };
        let anti_lobe = if anti_lobe_bank_path.exists() {
            RWIFCrystal::load_from_path(&anti_lobe_bank_path)?
        } else {
            RWIFCrystal {
                id: "anti_lobe".to_string(),
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

        let relation_registry = grammar.relation_registry();

        let mut agent = CSIFAgent {
            cache: QueryCache::new(),
            index,
            crystal,
            anti_lobe,
            bank_path: bank_path.to_path_buf(),
            anti_lobe_bank_path,
            grammar,
            relation_registry,
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
        self.anti_lobe.save_to_path(&self.anti_lobe_bank_path)?;
        Ok(())
    }

    pub fn anti_lobe_snapshot(&self) -> AntiLobeSnapshot {
        let mut entries = self
            .anti_lobe
            .edges
            .values()
            .filter_map(|edge| {
                let subject = node_label_by_id(&self.anti_lobe, &edge.source)?.to_string();
                let object = node_label_by_id(&self.anti_lobe, &edge.target)?.to_string();
                let last_event = edge.trajectory.last();
                Some(AntiLobeEntry {
                    relation: edge.relation.clone(),
                    subject,
                    object,
                    last_phase: last_event.map(|event| event.phase),
                    last_source_type: last_event.map(|event| event.source.source_type.clone()),
                    last_source_id: last_event.map(|event| event.source.source_id.clone()),
                    trajectory_len: edge.trajectory.len(),
                })
            })
            .collect::<Vec<_>>();

        entries.sort_by(|left, right| {
            (&left.subject, &left.relation, &left.object).cmp(&(&right.subject, &right.relation, &right.object))
        });

        AntiLobeSnapshot {
            bank_path: self.anti_lobe_bank_path.display().to_string(),
            entry_count: entries.len(),
            entries,
        }
    }

    /// Process a natural language query (or structured input)
    pub fn query(&mut self, input: &str) -> String {
        self.query_with_certificate(input).answer
    }

    pub fn explain_query(&self, input: &str) -> ExplainResultPayload {
        if let Some(intent) = self.grammar.parse_query(input) {
            match &intent {
                QueryIntent::ConfirmRelation {
                    relation,
                    subject,
                    object,
                } => {
                    let route_audit = Some(self.route_audit_for_relation(relation, subject, object));
                    let relation_type = RelationType::from_str(relation.as_str());
                    let path = relation_type
                        .and_then(|rel| self.infer_relation_path(subject, object, rel))
                        .unwrap_or_default();
                    let confidence = relation_type
                        .and_then(|rel| self.path_confidence(&path, rel))
                        .or_else(|| Some(if path.is_empty() { 0.0 } else { 1.0 }));
                    let depth_limit = relation_type
                        .and_then(|rel| self.relation_registry.spec_by_type(rel))
                        .and_then(|spec| spec.max_depth);

                    let considered_contradictions = if path.is_empty() {
                        vec![
                            "No supporting path found under current relation/depth policy.".to_string(),
                        ]
                    } else {
                        self.describe_path_contradictions(&path)
                    };

                    let answer = self
                        .answer_from_crystal(&intent)
                        .map(|resolution| format!("[CRYSTAL] {}", resolution.answer))
                        .unwrap_or_else(|| {
                            "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string()
                        });

                    ExplainResultPayload {
                        input: input.to_string(),
                        answer,
                        intent: "confirm_relation".to_string(),
                        relation: Some(relation.clone()),
                        depth_limit,
                        path,
                        considered_contradictions,
                        confidence,
                        request_time_context: Some(current_request_time_context()),
                        route_audit,
                    }
                }
                QueryIntent::Describe { .. } => {
                    let answer = self
                        .answer_from_crystal(&intent)
                        .map(|resolution| format!("[CRYSTAL] {}", resolution.answer))
                        .unwrap_or_else(|| {
                            "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string()
                        });
                    ExplainResultPayload {
                        input: input.to_string(),
                        answer,
                        intent: "describe_entity".to_string(),
                        relation: None,
                        depth_limit: None,
                        path: Vec::new(),
                        considered_contradictions: vec![
                            "No transitive relation path was required for describe intent.".to_string(),
                        ],
                        confidence: None,
                        request_time_context: Some(current_request_time_context()),
                        route_audit: None,
                    }
                }
                QueryIntent::ComputeExpression { .. } => {
                    let answer = self
                        .answer_from_crystal(&intent)
                        .map(|resolution| format!("[CRYSTAL] {}", resolution.answer))
                        .unwrap_or_else(|| {
                            "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string()
                        });
                    ExplainResultPayload {
                        input: input.to_string(),
                        answer,
                        intent: "compute_expression".to_string(),
                        relation: None,
                        depth_limit: None,
                        path: Vec::new(),
                        considered_contradictions: vec![
                            "Compute intent is arithmetic; no graph contradiction path was evaluated.".to_string(),
                        ],
                        confidence: Some(1.0),
                        request_time_context: Some(current_request_time_context()),
                        route_audit: None,
                    }
                }
                QueryIntent::SolveEquation { .. } => {
                    let answer = self
                        .answer_from_crystal(&intent)
                        .map(|resolution| format!("[CRYSTAL] {}", resolution.answer))
                        .unwrap_or_else(|| {
                            "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string()
                        });
                    ExplainResultPayload {
                        input: input.to_string(),
                        answer,
                        intent: "solve_equation".to_string(),
                        relation: None,
                        depth_limit: None,
                        path: Vec::new(),
                        considered_contradictions: vec![
                            "Solve intent uses symbolic math proof, not graph contradiction checks.".to_string(),
                        ],
                        confidence: Some(1.0),
                        request_time_context: Some(current_request_time_context()),
                        route_audit: None,
                    }
                }
            }
        } else {
            ExplainResultPayload {
                input: input.to_string(),
                answer: "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string(),
                intent: "unknown".to_string(),
                relation: None,
                depth_limit: None,
                path: Vec::new(),
                considered_contradictions: vec![
                    "Unable to parse input into a supported query intent.".to_string(),
                ],
                confidence: None,
                request_time_context: Some(current_request_time_context()),
                route_audit: None,
            }
        }
    }

    pub fn query_with_certificate(&mut self, input: &str) -> QueryResultPayload {
        if self.index_dirty {
            self.index.index_crystal(&self.crystal);
            self.index_dirty = false;
        }

        let clauses = split_compound_query(input);
        if clauses.len() > 1 {
            let clause_results = clauses
                .into_iter()
                .map(|clause| {
                    let result = self.query_single_with_certificate(&clause);
                    let semantic_projection =
                        semantic_projection_for_clause(&clause, &result.answer, result.certificate.as_ref());
                    QueryClauseResult {
                        input: clause,
                        answer: result.answer,
                        certificate: result.certificate,
                        semantic_projection,
                        route_audit: result.route_audit,
                    }
                })
                .collect::<Vec<_>>();

            let answer = clause_results
                .iter()
                .map(|entry| entry.answer.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let clause_count = clause_results.len();

            let clauses_with_certificates = clause_results
                .iter()
                .filter(|entry| entry.certificate.is_some())
                .count();
            let verified_certificates = clause_results
                .iter()
                .filter_map(|entry| entry.certificate.as_ref())
                .filter(|certificate| verify_proof_certificate(certificate))
                .count();
            let all_clause_certificates_verified = clauses_with_certificates > 0
                && clauses_with_certificates == verified_certificates;
            let mut intents = clause_results
                .iter()
                .map(|entry| entry.semantic_projection.intent.clone())
                .collect::<Vec<_>>();
            intents.sort();
            intents.dedup();

            let mut meaning_vocabulary = clause_results
                .iter()
                .flat_map(|entry| entry.semantic_projection.meaning_tokens.clone())
                .collect::<Vec<_>>();
            meaning_vocabulary.sort();
            meaning_vocabulary.dedup();

            return QueryResultPayload {
                answer,
                certificate: None,
                clauses: Some(clause_results),
                composite: Some(CompositeQuerySummary {
                    clause_count,
                    clauses_with_certificates,
                    verified_certificates,
                    all_clause_certificates_verified,
                    intents,
                    meaning_vocabulary,
                }),
                request_time_context: Some(current_request_time_context()),
                route_audit: None,
            };
        }

        self.query_single_with_certificate(input)
    }

    fn query_single_with_certificate(&mut self, input: &str) -> QueryResultPayload {

        let Some(intent) = self.grammar.parse_query(input) else {
            if let Some(certificate) = build_fallback_language_certificate(input) {
                let answer = if let Some(trigger) = certificate.parse.clarification_trigger.as_ref() {
                    format!("[NEEDS_INPUT] {}", trigger.question)
                } else {
                    self.answer_for_fallback_language_certificate(&certificate)
                };
                return QueryResultPayload {
                    answer,
                    certificate: Some(ProofCertificate::Language(certificate)),
                    clauses: None,
                    composite: None,
                    request_time_context: Some(current_request_time_context()),
                    route_audit: None,
                };
            }
            return QueryResultPayload {
                answer: "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string(),
                certificate: None,
                clauses: None,
                composite: None,
                request_time_context: Some(current_request_time_context()),
                route_audit: None,
            };
        };

        let mut route_audit = None;
        if let QueryIntent::ConfirmRelation {
            relation,
            subject,
            object,
        } = &intent
        {
            if let Some(relation_type) = RelationType::from_str(relation) {
                let (path, trace) = self.infer_relation_path_with_trace(subject, object, relation_type);
                if path.is_none() && !self.has_explicit_negative_relation(subject, object, relation_type) {
                    route_audit = Some(RouteAuditTrail {
                        relation: Some(relation.clone()),
                        subject: Some(subject.clone()),
                        object: Some(object.clone()),
                        tried: trace.tried.clone(),
                        stop_reason: trace.stop_reason.clone(),
                        negative_evidence: self.negative_evidence_for_relation(subject, object, relation_type),
                        anti_lobe_bank_path: self
                            .has_explicit_negative_relation(subject, object, relation_type)
                            .then(|| self.anti_lobe_bank_path.display().to_string()),
                    });
                    self.persist_negative_relation(subject, object, relation_type, &trace.stop_reason);
                    self.commit_pending_side_effects();
                }
            }

            if route_audit.is_none() {
                route_audit = Some(self.route_audit_for_relation(relation, subject, object));
            }
        }

        if let Some(resolution) = self.answer_from_crystal(&intent) {
            let query_phase = 0.0;
            let query_sigma = 0.02;
            let cache_key = cache_key_for_intent(&intent);
            let cached = CachedResponse {
                response: resolution.answer.clone(),
                resonance: 0.0,
                sigma: query_sigma,
                candidate_node_id: format!("n_{}", slug(subject_hint_for_intent(&intent))),
            };
            self.cache.insert(&cache_key, query_phase, query_sigma, cached);
            return QueryResultPayload {
                answer: format!("[CRYSTAL] {}", resolution.answer),
                certificate: resolution.certificate,
                clauses: None,
                composite: None,
                request_time_context: Some(current_request_time_context()),
                route_audit: route_audit.clone(),
            };
        }

        if matches!(intent, QueryIntent::Describe { .. }) {
            return QueryResultPayload {
                answer: "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string(),
                certificate: None,
                clauses: None,
                composite: None,
                request_time_context: Some(current_request_time_context()),
                route_audit: route_audit.clone(),
            };
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
            PreflightResult::ShortCircuit(response) => QueryResultPayload {
                answer: format!("[CACHE] {}", response.response),
                certificate: None,
                clauses: None,
                composite: None,
                request_time_context: Some(current_request_time_context()),
                route_audit: route_audit.clone(),
            },
            PreflightResult::CacheMiss | PreflightResult::NeedsDeepValidation => QueryResultPayload {
                answer: "[NEEDS_INPUT] I don't have that knowledge yet. Please teach me.".to_string(),
                certificate: None,
                clauses: None,
                composite: None,
                request_time_context: Some(current_request_time_context()),
                route_audit,
            },
        }
    }

    fn answer_for_fallback_language_certificate(&mut self, certificate: &LanguageCertificate) -> String {
        match &certificate.replay {
            LanguageCertificateReplay::InstructionRequest {
                action,
                target,
                ambiguous: _,
                negated: _,
                plan_steps,
                safe_actions,
                execution_policy,
                dry_run_results,
            } => {
                let target_text = target
                    .as_ref()
                    .map(|v| format!(" for {}", v))
                    .unwrap_or_default();
                let plan = plan_steps
                    .iter()
                    .enumerate()
                    .map(|(idx, step)| format!("{}. {}", idx + 1, step))
                    .collect::<Vec<_>>()
                    .join(" ");
                let actions = safe_actions
                    .iter()
                    .map(|action| format!("{}: {}", action.kind, action.command))
                    .collect::<Vec<_>>()
                    .join(" | ");
                let simulation = dry_run_results
                    .iter()
                    .map(|result| {
                        format!(
                            "{}:{} ({})",
                            result.kind,
                            if result.allowed { "ok" } else { "blocked" },
                            result.reason
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                format!(
                    "[CRYSTAL] [PLAN] action={}{}; {} Safe actions: {} Dry-run[{}]: {}",
                    action, target_text, plan, actions, execution_policy.mode, simulation
                )
            }
            LanguageCertificateReplay::NarrativeState {
                subject,
                state,
                negated,
            } => {
                self.persist_language_temporal_state(subject, state, *negated);
                self.commit_pending_side_effects();
                if *negated {
                    format!(
                        "[CRYSTAL] [TEACHING] Narrative state persisted: {} is not {}.",
                        subject, state
                    )
                } else {
                    format!(
                        "[CRYSTAL] [TEACHING] Narrative state persisted: {} is {}.",
                        subject, state
                    )
                }
            }
            LanguageCertificateReplay::NarrativeEvent {
                actor,
                event,
                object,
                negated,
            } => {
                self.persist_language_temporal_event(actor, event, object.as_deref(), *negated);
                self.commit_pending_side_effects();
                if let Some(object) = object {
                    if *negated {
                        format!(
                            "[CRYSTAL] [TEACHING] Narrative event persisted: {} did not {} {}.",
                            actor, event, object
                        )
                    } else {
                        format!(
                            "[CRYSTAL] [TEACHING] Narrative event persisted: {} {} {}.",
                            actor, event, object
                        )
                    }
                } else if *negated {
                    format!(
                        "[CRYSTAL] [TEACHING] Narrative event persisted: {} did not {}.",
                        actor, event
                    )
                } else {
                    format!(
                        "[CRYSTAL] [TEACHING] Narrative event persisted: {} {}.",
                        actor, event
                    )
                }
            }
            _ => {
                "[NEEDS_INPUT] I parsed the request, but I do not have enough grounded material yet. Please teach me or clarify the task.".to_string()
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
    fn answer_from_crystal(&self, intent: &QueryIntent) -> Option<CrystalAnswer> {
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

                    return Some(CrystalAnswer {
                        answer: response,
                        certificate: Some(ProofCertificate::Language(
                            build_language_describe_certificate(
                                subject,
                                is_a_targets.clone(),
                                properties.clone(),
                                subtypes.clone(),
                            ),
                        )),
                    });
                }
                None
            }
            QueryIntent::ConfirmRelation {
                relation,
                subject,
                object,
            } => {
                let Some(relation_type) = RelationType::from_str(relation) else {
                    return Some(CrystalAnswer {
                        answer: format!(
                            "NO: relation '{}' is not registered for inference.",
                            relation
                        ),
                        certificate: None,
                    });
                };

                let witness_path = self.infer_relation_path(subject, object, relation_type);
                if witness_path.is_some() {
                    Some(CrystalAnswer {
                        answer: format_relation_confirmation(subject, object, relation_type, true),
                        certificate: Some(ProofCertificate::Language(
                            build_language_confirm_certificate(
                                relation_type,
                                subject,
                                object,
                                true,
                                witness_path.unwrap_or_default(),
                            ),
                        )),
                    })
                } else {
                    Some(CrystalAnswer {
                        answer: format_relation_confirmation(subject, object, relation_type, false),
                        certificate: Some(ProofCertificate::Language(
                            build_language_confirm_certificate(
                                relation_type,
                                subject,
                                object,
                                false,
                                Vec::new(),
                            ),
                        )),
                    })
                }
            }
            QueryIntent::ComputeExpression { expression } => {
                Some(CrystalAnswer {
                    answer: render_compute_expression(expression),
                    certificate: None,
                })
            }
            QueryIntent::SolveEquation { equation } => {
                let solution = solve_equation(equation);
                Some(CrystalAnswer {
                    answer: render_solve_equation(equation, &solution),
                    certificate: solve_certificate_from_solution(&solution),
                })
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

        self.persist_positive_relation(&fact.subject, &fact.object, relation_type, "teach", "local");
        self.remove_explicit_negative_relation(&fact.subject, &fact.object, relation_type);
        self.index_dirty = true;
        true
    }

    fn persist_positive_relation(
        &mut self,
        subject: &str,
        object: &str,
        relation_type: RelationType,
        source_type: &str,
        source_id: &str,
    ) {
        let subject_id = ensure_node(&mut self.crystal, subject);
        let object_id = ensure_node(&mut self.crystal, object);
        let edge_id = format!("e_{}_{}_{}", slug(subject), relation_type.as_str(), slug(object));

        let event = PhaseEvent {
            timestamp: Utc::now(),
            phase: 0.0,
            sigma: 0.02,
            source: Provenance {
                source_type: source_type.to_string(),
                source_id: source_id.to_string(),
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
    }

    fn persist_language_temporal_state(&mut self, subject: &str, state: &str, negated: bool) {
        let relation = if negated { "state_not_at" } else { "state_at" };
        let state_label = if negated {
            format!("not:{}", state)
        } else {
            state.to_string()
        };
        self.persist_temporal_edge(subject, relation, &state_label, "Language");
    }

    fn persist_language_temporal_event(
        &mut self,
        actor: &str,
        event: &str,
        object: Option<&str>,
        negated: bool,
    ) {
        let relation = if negated { "event_not_at" } else { "event_at" };
        let event_label = if negated {
            format!("not:{}", event)
        } else {
            event.to_string()
        };
        self.persist_temporal_edge(actor, relation, &event_label, "Language");

        if let Some(object) = object {
            self.persist_temporal_edge(&event_label, "event_object", object, "Language");
        }
    }

    fn persist_temporal_edge(&mut self, source_label: &str, relation: &str, target_label: &str, lobe: &str) {
        let source_id = ensure_node(&mut self.crystal, source_label);
        let target_id = ensure_node(&mut self.crystal, target_label);

        let event = PhaseEvent {
            timestamp: Utc::now(),
            phase: 0.0,
            sigma: 0.02,
            source: Provenance {
                source_type: "query_parse".to_string(),
                source_id: "language_fallback".to_string(),
            },
        };

        let edge_id = format!(
            "e_{}_{}_{}",
            slug(source_label),
            relation,
            slug(target_label)
        );

        if let Some(edge) = self.crystal.edges.get_mut(&edge_id) {
            edge.trajectory.push(event.clone());
        } else {
            self.crystal.edges.insert(
                edge_id.clone(),
                RWIFEdge {
                    edge_id,
                    source: source_id,
                    relation: relation.to_string(),
                    target: target_id.clone(),
                    lobe: lobe.to_string(),
                    trajectory: vec![event.clone()],
                },
            );
        }

        // Explicit time anchor edge keeps temporal evidence queryable in RWIF.
        let time_label = format!("time:{}", event.timestamp.to_rfc3339());
        let time_id = ensure_node(&mut self.crystal, &time_label);
        let time_edge_id = format!(
            "e_{}_{}_{}",
            slug(target_label),
            "observed_at",
            slug(&time_label)
        );
        if let Some(edge) = self.crystal.edges.get_mut(&time_edge_id) {
            edge.trajectory.push(event.clone());
        } else {
            self.crystal.edges.insert(
                time_edge_id.clone(),
                RWIFEdge {
                    edge_id: time_edge_id,
                    source: target_id,
                    relation: "observed_at".to_string(),
                    target: time_id,
                    lobe: lobe.to_string(),
                    trajectory: vec![event],
                },
            );
        }

        self.index_dirty = true;
    }

    fn explicit_negative_edge(&self, subject: &str, object: &str, relation: RelationType) -> Option<&RWIFEdge> {
        let relation_name = format!("not_{}", relation.as_str());
        self.anti_lobe.edges.values().find(|edge| {
            if edge.relation != relation_name {
                return false;
            }

            let Some(source_label) = node_label_by_id(&self.anti_lobe, &edge.source) else {
                return false;
            };
            let Some(target_label) = node_label_by_id(&self.anti_lobe, &edge.target) else {
                return false;
            };

            source_label == subject && target_label == object
        })
    }

    fn has_explicit_negative_relation(&self, subject: &str, object: &str, relation: RelationType) -> bool {
        self.explicit_negative_edge(subject, object, relation).is_some()
    }

    fn remove_explicit_negative_relation(&mut self, subject: &str, object: &str, relation: RelationType) {
        let anti_relation = format!("not_{}", relation.as_str());
        let edge_id = self.anti_lobe.edges.iter().find_map(|(edge_id, edge)| {
            if edge.relation != anti_relation {
                return None;
            }

            let source_label = node_label_by_id(&self.anti_lobe, &edge.source)?;
            let target_label = node_label_by_id(&self.anti_lobe, &edge.target)?;
            (source_label == subject && target_label == object).then(|| edge_id.clone())
        });

        if let Some(edge_id) = edge_id {
            self.anti_lobe.edges.remove(&edge_id);
        }
    }

    fn persist_negative_relation(
        &mut self,
        subject: &str,
        object: &str,
        relation: RelationType,
        reason: &str,
    ) {
        let subject_id = ensure_node(&mut self.anti_lobe, subject);
        let object_id = ensure_node(&mut self.anti_lobe, object);
        let anti_relation = format!("not_{}", relation.as_str());
        let edge_id = format!("e_{}_{}_{}", slug(subject), anti_relation, slug(object));
        let event = PhaseEvent {
            timestamp: Utc::now(),
            phase: PI,
            sigma: 0.02,
            source: Provenance {
                source_type: "anti_lobe".to_string(),
                source_id: reason.to_string(),
            },
        };

        if let Some(edge) = self.anti_lobe.edges.get_mut(&edge_id) {
            edge.trajectory.push(event);
        } else {
            self.anti_lobe.edges.insert(
                edge_id.clone(),
                RWIFEdge {
                    edge_id,
                    source: subject_id,
                    relation: anti_relation,
                    target: object_id,
                    lobe: "AntiLobe".to_string(),
                    trajectory: vec![event],
                },
            );
        }

    }

    fn commit_pending_side_effects(&mut self) {
        self.pending_saves = self.pending_saves.saturating_add(1);
        if self.pending_saves >= self.save_every {
            let save_started = Instant::now();
            self.save().ok();
            log_play_phase_if_slow("play.commit_pending_side_effects.save", save_started, || {
                format!("save_every={} pending_saves_before_reset={}", self.save_every, self.pending_saves)
            });
            self.pending_saves = 0;
        }
    }

    #[allow(dead_code)]
    fn infer_relation(&self, subject: &str, object: &str, relation: RelationType) -> bool {
        self.infer_relation_path(subject, object, relation).is_some()
    }

    fn infer_relation_path_with_trace(
        &self,
        subject: &str,
        object: &str,
        relation: RelationType,
    ) -> (Option<Vec<String>>, RelationInferenceTrace) {
        let Some(spec) = self.relation_registry.spec_by_type(relation) else {
            return (
                None,
                RelationInferenceTrace {
                    tried: Vec::new(),
                    stop_reason: "relation_not_registered".to_string(),
                },
            );
        };

        if self.has_explicit_negative_relation(subject, object, relation) {
            return (
                None,
                RelationInferenceTrace {
                    tried: vec![format!(
                        "{} -not_{}-> {}",
                        subject,
                        relation.as_str(),
                        object
                    )],
                    stop_reason: "anti_lobe_negative_match".to_string(),
                },
            );
        }

        if !spec.transitive {
            let direct_targets = self.direct_targets_for_relation(subject, relation);
            let mut tried = direct_targets
                .iter()
                .map(|target| format!("{} -{}-> {}", subject, relation.as_str(), target))
                .collect::<Vec<_>>();
            tried.sort();
            tried.dedup();
            let path = direct_targets
                .iter()
                .any(|target| target == object)
                .then(|| vec![subject.to_string(), object.to_string()]);
            let stop_reason = if path.is_some() {
                "direct_match_found"
            } else {
                "direct_no_match"
            }
            .to_string();
            return (path, RelationInferenceTrace { tried, stop_reason });
        }

        let max_depth = spec.max_depth;

        let mut visited = HashSet::new();
        let mut previous = HashMap::<String, String>::new();
        let mut queue = VecDeque::from([(subject.to_string(), 0usize)]);
        let mut tried = Vec::new();
        let mut depth_limited = false;

        while let Some((current, depth)) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }

            if max_depth.is_some_and(|limit| depth >= limit) {
                depth_limited = true;
                continue;
            }

            for next in self.direct_targets_for_relation(&current, relation) {
                tried.push(format!(
                    "{} -{}-> {} (depth {})",
                    current,
                    relation.as_str(),
                    next,
                    depth + 1
                ));

                if next == object {
                    previous.insert(next.clone(), current.clone());
                    let mut path = vec![object.to_string()];
                    let mut cursor = object.to_string();
                    while let Some(prev) = previous.get(&cursor).cloned() {
                        path.push(prev.clone());
                        if prev == subject {
                            break;
                        }
                        cursor = prev;
                    }
                    path.reverse();
                    return (
                        Some(path),
                        RelationInferenceTrace {
                            tried,
                            stop_reason: "path_found".to_string(),
                        },
                    );
                }
                if !visited.contains(&next) {
                    previous.entry(next.clone()).or_insert_with(|| current.clone());
                    queue.push_back((next, depth + 1));
                }
            }
        }

        let stop_reason = if depth_limited {
            match max_depth {
                Some(limit) => format!("stopped_at_depth_limit:{}", limit),
                None => "stopped_at_depth_limit".to_string(),
            }
        } else {
            "no_supporting_path".to_string()
        };

        (None, RelationInferenceTrace { tried, stop_reason })
    }

    fn infer_relation_path(&self, subject: &str, object: &str, relation: RelationType) -> Option<Vec<String>> {
        self.infer_relation_path_with_trace(subject, object, relation).0
    }

    fn negative_evidence_for_relation(
        &self,
        subject: &str,
        object: &str,
        relation: RelationType,
    ) -> Vec<String> {
        let mut evidence = Vec::new();

        if let Some(edge) = self.explicit_negative_edge(subject, object, relation) {
            if let Some(event) = edge.trajectory.last() {
                evidence.push(format!(
                    "Explicit AntiLobe edge observed on {} -{}-> {} (phase {:.3}, source {}).",
                    subject,
                    edge.relation,
                    object,
                    event.phase,
                    event.source.source_type
                ));
            }
        }

        if let Some(phase) = self.last_phase_for_edge(subject, object, relation) {
            if phase.abs() > (PI - 0.1) {
                evidence.push(format!(
                    "High anti-phase edge observed on {} -{}-> {} (phase {:.3}).",
                    subject,
                    relation.as_str(),
                    object,
                    phase
                ));
            }
        }

        let reverse_exists = self
            .direct_targets_for_relation(object, relation)
            .iter()
            .any(|target| target == subject);
        if reverse_exists {
            evidence.push(format!(
                "Reverse direction exists: {} -{}-> {}.",
                object,
                relation.as_str(),
                subject
            ));
        }

        if evidence.is_empty() {
            evidence.push(
                "No direct anti-phase or reverse-direction evidence observed for this relation."
                    .to_string(),
            );
        }

        evidence
    }

    fn route_audit_for_relation(
        &self,
        relation: &str,
        subject: &str,
        object: &str,
    ) -> RouteAuditTrail {
        if let Some(relation_type) = RelationType::from_str(relation) {
            let (_, trace) = self.infer_relation_path_with_trace(subject, object, relation_type);
            RouteAuditTrail {
                relation: Some(relation.to_string()),
                subject: Some(subject.to_string()),
                object: Some(object.to_string()),
                tried: trace.tried,
                stop_reason: trace.stop_reason,
                negative_evidence: self.negative_evidence_for_relation(subject, object, relation_type),
                anti_lobe_bank_path: self
                    .has_explicit_negative_relation(subject, object, relation_type)
                    .then(|| self.anti_lobe_bank_path.display().to_string()),
            }
        } else {
            RouteAuditTrail {
                relation: Some(relation.to_string()),
                subject: Some(subject.to_string()),
                object: Some(object.to_string()),
                tried: Vec::new(),
                stop_reason: "relation_not_registered".to_string(),
                negative_evidence: vec![
                    "Relation is not registered for inference; no route attempts were executed."
                        .to_string(),
                ],
                anti_lobe_bank_path: None,
            }
        }
    }

    fn path_confidence(&self, path: &[String], relation: RelationType) -> Option<f64> {
        if path.len() < 2 {
            return None;
        }

        let mut scores = Vec::new();
        for pair in path.windows(2) {
            let [source, target] = pair else {
                continue;
            };
            if let Some(phase) = self.last_phase_for_edge(source, target, relation) {
                let normalized = 1.0 - (phase.abs() / PI).min(1.0);
                scores.push(normalized.max(0.0));
            }
        }

        if scores.is_empty() {
            None
        } else {
            Some(scores.iter().sum::<f64>() / (scores.len() as f64))
        }
    }

    fn last_phase_for_edge(&self, source: &str, target: &str, relation: RelationType) -> Option<f64> {
        self.crystal
            .edges
            .values()
            .find_map(|edge| {
                if edge.relation != relation.as_str() {
                    return None;
                }
                let source_label = node_label_by_id(&self.crystal, &edge.source)?;
                let target_label = node_label_by_id(&self.crystal, &edge.target)?;
                if source_label != source || target_label != target {
                    return None;
                }
                edge.trajectory.last().map(|event| event.phase)
            })
    }

    fn describe_path_contradictions(&self, path: &[String]) -> Vec<String> {
        if path.len() < 2 {
            return vec!["No path evidence available for contradiction review.".to_string()];
        }

        let mut messages = Vec::new();
        for pair in path.windows(2) {
            let [source, target] = pair else {
                continue;
            };
            let high_phase = self
                .crystal
                .edges
                .values()
                .filter_map(|edge| {
                    let source_label = node_label_by_id(&self.crystal, &edge.source)?;
                    let target_label = node_label_by_id(&self.crystal, &edge.target)?;
                    if source_label != source || target_label != target {
                        return None;
                    }
                    edge.trajectory.last().map(|event| (edge.relation.clone(), event.phase))
                })
                .filter(|(_, phase)| phase.abs() > (PI - 0.1))
                .collect::<Vec<_>>();

            if high_phase.is_empty() {
                messages.push(format!(
                    "No high-phase contradiction observed on edge {} -> {}.",
                    source, target
                ));
            } else {
                for (relation, phase) in high_phase {
                    messages.push(format!(
                        "Potential contradiction on relation {} for {} -> {} (phase {:.3}).",
                        relation, source, target, phase
                    ));
                }
            }
        }

        messages
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

    fn direct_targets_index_for_relation(
        &self,
        relation: RelationType,
    ) -> HashMap<String, Vec<String>> {
        let mut targets_by_subject = HashMap::<String, Vec<String>>::new();
        for edge in self.crystal.edges.values() {
            if edge.relation != relation.as_str() {
                continue;
            }

            let Some(source_label) = node_label_by_id(&self.crystal, &edge.source) else {
                continue;
            };
            let Some(target_label) = node_label_by_id(&self.crystal, &edge.target) else {
                continue;
            };

            targets_by_subject
                .entry(source_label.to_string())
                .or_default()
                .push(target_label.to_string());
        }

        for targets in targets_by_subject.values_mut() {
            targets.sort();
            targets.dedup();
        }

        targets_by_subject
    }

    pub fn run_play_cycle(&mut self) -> Vec<PlayAttempt> {
        let cycle_started = Instant::now();
        let mut attempts = Vec::new();

        let transitive_candidate_started = Instant::now();
        let transitive_candidate = self.next_transitive_play_candidate();
        log_play_phase_if_slow("next_transitive_play_candidate", transitive_candidate_started, || {
            format!("found_candidate={}", transitive_candidate.is_some())
        });
        if let Some(candidate) = transitive_candidate {
            let subject = candidate.subject.clone();
            let object = candidate.object.clone();
            let relation = candidate.relation.clone();
            let transitive_play_started = Instant::now();
            let attempt = self.play_transitive_candidate(candidate);
            log_play_phase_if_slow("play_transitive_candidate", transitive_play_started, || {
                format!(
                    "relation={} subject={} object={} outcome={:?}",
                    relation, subject, object, attempt.outcome
                )
            });
            attempts.push(attempt);
        }

        let property_candidate_started = Instant::now();
        let property_candidate = self.next_property_play_candidate();
        log_play_phase_if_slow("next_property_play_candidate", property_candidate_started, || {
            format!("found_candidate={}", property_candidate.is_some())
        });
        if let Some(candidate) = property_candidate {
            let subject = candidate.subject.clone();
            let object = candidate.object.clone();
            let property_play_started = Instant::now();
            let attempt = self.play_property_candidate(candidate);
            log_play_phase_if_slow("play_property_candidate", property_play_started, || {
                format!(
                    "subject={} object={} outcome={:?}",
                    subject, object, attempt.outcome
                )
            });
            attempts.push(attempt);
        }

        log_play_phase_if_slow("run_play_cycle", cycle_started, || {
            format!("attempt_count={}", attempts.len())
        });

        attempts
    }

    pub fn preview_play_cycle(&self) -> Vec<PlayAttempt> {
        let cycle_started = Instant::now();
        let mut attempts = Vec::new();

        if play_trace_slow_ms().is_some() {
            eprintln!("play-trace phase=preview_play_cycle.enter");
        }

        if play_trace_slow_ms().is_some() {
            eprintln!("play-trace phase=preview_play_cycle.before_next_transitive");
        }
        let transitive_candidate_started = Instant::now();
        let transitive_candidate = self.next_transitive_play_candidate();
        log_play_phase_if_slow("preview.next_transitive_play_candidate", transitive_candidate_started, || {
            format!("found_candidate={}", transitive_candidate.is_some())
        });
        if play_trace_slow_ms().is_some() {
            eprintln!("play-trace phase=preview_play_cycle.after_next_transitive");
        }
        if let Some(mut candidate) = transitive_candidate {
            let relation_type = RelationType::from_str(&candidate.relation)
                .expect("known transitive play relation");
            if play_trace_slow_ms().is_some() {
                eprintln!(
                    "play-trace phase=preview_play_cycle.before_transitive_eval relation={} subject={} object={}",
                    candidate.relation,
                    candidate.subject,
                    candidate.object
                );
            }
            if self
                .direct_targets_for_relation(&candidate.subject, relation_type)
                .iter()
                .any(|target| target == &candidate.object)
            {
                candidate.outcome = PlayAttemptOutcome::SkippedKnownSuccess;
                candidate.detail = "direct edge already crystallized".to_string();
            } else if self
                .infer_relation_path(&candidate.subject, &candidate.object, relation_type)
                .is_some()
            {
                candidate.outcome = PlayAttemptOutcome::SuccessCrystallized;
                candidate.detail =
                    "transitive path verified (preview only; no crystallization write)".to_string();
            } else {
                candidate.outcome = PlayAttemptOutcome::FailurePersisted;
                candidate.detail =
                    "candidate path not supported (preview only; no anti-lobe write)".to_string();
            }
            if play_trace_slow_ms().is_some() {
                eprintln!(
                    "play-trace phase=preview_play_cycle.after_transitive_eval outcome={:?}",
                    candidate.outcome
                );
            }
            attempts.push(candidate);
        }

        if play_trace_slow_ms().is_some() {
            eprintln!("play-trace phase=preview_play_cycle.before_next_property");
        }
        let property_candidate_started = Instant::now();
        let property_candidate = self.next_property_play_candidate();
        log_play_phase_if_slow("preview.next_property_play_candidate", property_candidate_started, || {
            format!("found_candidate={}", property_candidate.is_some())
        });
        if play_trace_slow_ms().is_some() {
            eprintln!("play-trace phase=preview_play_cycle.after_next_property");
        }
        if let Some(mut candidate) = property_candidate {
            let relation_type = RelationType::HasProperty;
            if play_trace_slow_ms().is_some() {
                eprintln!(
                    "play-trace phase=preview_play_cycle.before_property_eval subject={} object={}",
                    candidate.subject,
                    candidate.object
                );
            }
            if self.has_explicit_negative_relation(&candidate.subject, &candidate.object, relation_type)
            {
                candidate.outcome = PlayAttemptOutcome::SkippedKnownFailure;
                candidate.detail = format!(
                    "suppressed by Anti-Lobe bank {}",
                    self.anti_lobe_bank_path.display()
                );
            } else if self
                .infer_relation_path(&candidate.subject, &candidate.object, relation_type)
                .is_some()
            {
                candidate.outcome = PlayAttemptOutcome::SuccessCrystallized;
                candidate.detail = "property candidate already supported (preview only)".to_string();
            } else {
                candidate.outcome = PlayAttemptOutcome::FailurePersisted;
                candidate.detail =
                    "property inheritance hypothesis failed (preview only; no anti-lobe write)"
                        .to_string();
            }
            if play_trace_slow_ms().is_some() {
                eprintln!(
                    "play-trace phase=preview_play_cycle.after_property_eval outcome={:?}",
                    candidate.outcome
                );
            }
            attempts.push(candidate);
        }

        log_play_phase_if_slow("preview_play_cycle", cycle_started, || {
            format!("attempt_count={}", attempts.len())
        });

        attempts
    }

    fn next_transitive_play_candidate(&self) -> Option<PlayAttempt> {
        let mut best_candidate: Option<PlayAttempt> = None;
        for relation in [RelationType::IsA, RelationType::Causes] {
            let targets_by_subject = self.direct_targets_index_for_relation(relation);
            for edge in self.crystal.edges.values() {
                if edge.relation != relation.as_str() {
                    continue;
                }
                let Some(subject) = node_label_by_id(&self.crystal, &edge.source) else {
                    continue;
                };
                let Some(middle) = node_label_by_id(&self.crystal, &edge.target) else {
                    continue;
                };

                let Some(middle_targets) = targets_by_subject.get(middle) else {
                    continue;
                };
                let subject_targets = targets_by_subject.get(subject);

                for object in middle_targets {
                    if subject == object {
                        continue;
                    }
                    if subject_targets
                        .is_some_and(|targets| targets.iter().any(|target| target == object))
                    {
                        continue;
                    }
                    let candidate = PlayAttempt {
                        relation: relation.as_str().to_string(),
                        subject: subject.to_string(),
                        object: object.clone(),
                        basis: vec![subject.to_string(), middle.to_string()],
                        outcome: PlayAttemptOutcome::SkippedKnownSuccess,
                        detail: String::new(),
                    };

                    let replace_best = best_candidate.as_ref().is_none_or(|current| {
                        (&candidate.relation, &candidate.subject, &candidate.object)
                            < (&current.relation, &current.subject, &current.object)
                    });
                    if replace_best {
                        best_candidate = Some(candidate);
                    }
                }
            }
        }

        best_candidate
    }

    fn play_transitive_candidate(&mut self, mut candidate: PlayAttempt) -> PlayAttempt {
        let relation_type = RelationType::from_str(&candidate.relation).expect("known transitive play relation");
        let direct_check_started = Instant::now();
        let direct_targets = self.direct_targets_for_relation(&candidate.subject, relation_type);
        log_play_phase_if_slow("play_transitive.direct_targets", direct_check_started, || {
            format!(
                "subject={} relation={} direct_target_count={}",
                candidate.subject,
                relation_type.as_str(),
                direct_targets.len()
            )
        });
        if direct_targets
            .iter()
            .any(|target| target == &candidate.object)
        {
            candidate.outcome = PlayAttemptOutcome::SkippedKnownSuccess;
            candidate.detail = "direct edge already crystallized".to_string();
            return candidate;
        }

        let infer_started = Instant::now();
        let inferred_path = self.infer_relation_path(&candidate.subject, &candidate.object, relation_type);
        log_play_phase_if_slow("play_transitive.infer_relation_path", infer_started, || {
            format!(
                "subject={} object={} relation={} path_found={}",
                candidate.subject,
                candidate.object,
                relation_type.as_str(),
                inferred_path.is_some()
            )
        });
        if inferred_path.is_some() {
            let persist_started = Instant::now();
            self.persist_positive_relation(
                &candidate.subject,
                &candidate.object,
                relation_type,
                "play",
                "transitive_crystallization",
            );
            self.remove_explicit_negative_relation(&candidate.subject, &candidate.object, relation_type);
            self.index_dirty = true;
            self.commit_pending_side_effects();
            log_play_phase_if_slow("play_transitive.persist_success", persist_started, || {
                format!(
                    "subject={} object={} relation={}",
                    candidate.subject,
                    candidate.object,
                    relation_type.as_str()
                )
            });
            candidate.outcome = PlayAttemptOutcome::SuccessCrystallized;
            candidate.detail = "transitive path verified and crystallized as a direct edge".to_string();
            return candidate;
        }

        candidate.outcome = PlayAttemptOutcome::FailurePersisted;
        candidate.detail = "candidate path was not supported".to_string();
        let persist_started = Instant::now();
        self.persist_negative_relation(&candidate.subject, &candidate.object, relation_type, "play_transitive_no_support");
        self.commit_pending_side_effects();
        log_play_phase_if_slow("play_transitive.persist_failure", persist_started, || {
            format!(
                "subject={} object={} relation={}",
                candidate.subject,
                candidate.object,
                relation_type.as_str()
            )
        });
        candidate
    }

    fn next_property_play_candidate(&self) -> Option<PlayAttempt> {
        let mut best_candidate: Option<PlayAttempt> = None;
        let property_targets_by_subject = self.direct_targets_index_for_relation(RelationType::HasProperty);
        for edge in self.crystal.edges.values() {
            if edge.relation != RelationType::IsA.as_str() {
                continue;
            }
            let Some(subject) = node_label_by_id(&self.crystal, &edge.source) else {
                continue;
            };
            let Some(parent) = node_label_by_id(&self.crystal, &edge.target) else {
                continue;
            };

            let Some(parent_properties) = property_targets_by_subject.get(parent) else {
                continue;
            };
            let subject_properties = property_targets_by_subject.get(subject);

            for property in parent_properties {
                if subject_properties
                    .is_some_and(|targets| targets.iter().any(|target| target == property))
                {
                    continue;
                }
                let candidate = PlayAttempt {
                    relation: RelationType::HasProperty.as_str().to_string(),
                    subject: subject.to_string(),
                    object: property.clone(),
                    basis: vec![subject.to_string(), parent.to_string()],
                    outcome: PlayAttemptOutcome::SkippedKnownSuccess,
                    detail: String::new(),
                };

                let replace_best = best_candidate.as_ref().is_none_or(|current| {
                    (&candidate.subject, &candidate.object) < (&current.subject, &current.object)
                });
                if replace_best {
                    best_candidate = Some(candidate);
                }
            }
        }

        best_candidate
    }

    fn play_property_candidate(&mut self, mut candidate: PlayAttempt) -> PlayAttempt {
        let relation_type = RelationType::HasProperty;
        let negative_check_started = Instant::now();
        let has_negative = self.has_explicit_negative_relation(&candidate.subject, &candidate.object, relation_type);
        log_play_phase_if_slow("play_property.has_explicit_negative_relation", negative_check_started, || {
            format!(
                "subject={} object={} has_negative={}",
                candidate.subject,
                candidate.object,
                has_negative
            )
        });
        if has_negative {
            candidate.outcome = PlayAttemptOutcome::SkippedKnownFailure;
            candidate.detail = format!(
                "suppressed by Anti-Lobe bank {}",
                self.anti_lobe_bank_path.display()
            );
            return candidate;
        }

        let infer_started = Instant::now();
        let inferred_path = self.infer_relation_path(&candidate.subject, &candidate.object, relation_type);
        log_play_phase_if_slow("play_property.infer_relation_path", infer_started, || {
            format!(
                "subject={} object={} path_found={}",
                candidate.subject,
                candidate.object,
                inferred_path.is_some()
            )
        });
        if inferred_path.is_some() {
            candidate.outcome = PlayAttemptOutcome::SuccessCrystallized;
            candidate.detail = "property candidate was already supported".to_string();
            return candidate;
        }

        let persist_started = Instant::now();
        self.persist_negative_relation(
            &candidate.subject,
            &candidate.object,
            relation_type,
            "play_property_no_support",
        );
        self.commit_pending_side_effects();
        log_play_phase_if_slow("play_property.persist_failure", persist_started, || {
            format!("subject={} object={}", candidate.subject, candidate.object)
        });
        candidate.outcome = PlayAttemptOutcome::FailurePersisted;
        candidate.detail = "property inheritance hypothesis failed and was persisted to Anti-Lobe".to_string();
        candidate
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

#[derive(Debug, Clone)]
struct CrystalAnswer {
    answer: String,
    certificate: Option<ProofCertificate>,
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

fn build_language_confirm_certificate(
    relation: RelationType,
    subject: &str,
    object: &str,
    proved: bool,
    witness_path: Vec<String>,
) -> LanguageCertificate {
    LanguageCertificate {
        family: "language-confirm-relation".to_string(),
        modality: "text".to_string(),
        parse: build_language_confirm_parse_certificate(subject, relation, object),
        replay: LanguageCertificateReplay::ConfirmRelation {
            relation: relation.as_str().to_string(),
            subject: subject.to_string(),
            object: object.to_string(),
            proved,
            witness_path,
        },
    }
}

fn build_language_describe_certificate(
    subject: &str,
    direct_classes: Vec<String>,
    properties: Vec<String>,
    subtypes: Vec<String>,
) -> LanguageCertificate {
    LanguageCertificate {
        family: "language-describe-entity".to_string(),
        modality: "text".to_string(),
        parse: build_language_describe_parse_certificate(subject),
        replay: LanguageCertificateReplay::DescribeEntity {
            subject: subject.to_string(),
            direct_classes,
            properties,
            subtypes,
        },
    }
}

fn build_language_confirm_parse_certificate(
    subject: &str,
    relation: RelationType,
    object: &str,
) -> LanguageParseCertificate {
    LanguageParseCertificate {
        normalized_input: format!("{}|{}|{}", subject, relation.as_str(), object),
        best: SemanticFormV1 {
            version: 1,
            intent: SemanticIntent::ConfirmRelation,
            atoms: vec![
                SemanticAtom {
                    primitive: SemanticPrimitive::Query,
                    role: Some("intent".to_string()),
                    value: "confirm_relation".to_string(),
                },
                SemanticAtom {
                    primitive: SemanticPrimitive::Entity,
                    role: Some("subject".to_string()),
                    value: subject.to_string(),
                },
                SemanticAtom {
                    primitive: SemanticPrimitive::Relation,
                    role: Some("predicate".to_string()),
                    value: relation.as_str().to_string(),
                },
                SemanticAtom {
                    primitive: SemanticPrimitive::Entity,
                    role: Some("object".to_string()),
                    value: object.to_string(),
                },
            ],
        },
        alternatives: vec![],
        rejections: vec![
            ParseRejection {
                label: "describe-entity".to_string(),
                reason: "input includes an explicit predicate-object relation target".to_string(),
            },
            ParseRejection {
                label: "compute-expression".to_string(),
                reason: "input matches relation grammar, not arithmetic grammar".to_string(),
            },
        ],
        ambiguity_remaining: false,
        clarification_trigger: None,
    }
}

fn build_language_describe_parse_certificate(subject: &str) -> LanguageParseCertificate {
    LanguageParseCertificate {
        normalized_input: subject.to_string(),
        best: SemanticFormV1 {
            version: 1,
            intent: SemanticIntent::DescribeEntity,
            atoms: vec![
                SemanticAtom {
                    primitive: SemanticPrimitive::Query,
                    role: Some("intent".to_string()),
                    value: "describe".to_string(),
                },
                SemanticAtom {
                    primitive: SemanticPrimitive::Entity,
                    role: Some("subject".to_string()),
                    value: subject.to_string(),
                },
            ],
        },
        alternatives: vec![],
        rejections: vec![
            ParseRejection {
                label: "confirm-relation".to_string(),
                reason: "input does not specify an explicit predicate-object target".to_string(),
            },
            ParseRejection {
                label: "compute-expression".to_string(),
                reason: "input does not contain arithmetic structure".to_string(),
            },
        ],
        ambiguity_remaining: false,
        clarification_trigger: None,
    }
}

fn build_fallback_language_certificate(input: &str) -> Option<LanguageCertificate> {
    let mut normalized = input
        .trim()
        .trim_end_matches('.')
        .trim_end_matches('?')
        .trim()
        .to_lowercase();
    normalized = strip_leading_chat_markers(normalized.as_str()).to_string();
    if normalized.is_empty() {
        return None;
    }

    if let Some(rest) = normalized.strip_prefix("how do i ") {
        return Some(build_instruction_language_certificate(
            &normalized,
            rest.trim(),
            false,
            false,
        ));
    }

    if let Some(rest) = normalized.strip_prefix("how to ") {
        return Some(build_instruction_language_certificate(
            &normalized,
            rest.trim(),
            false,
            false,
        ));
    }

    if let Some((subject, state, negated)) = parse_narrative_state_candidate(&normalized) {
        return Some(build_narrative_state_language_certificate(
            &normalized,
            &subject,
            &state,
            negated,
        ));
    }

    if let Some((actor, event, object, negated)) = parse_narrative_event_candidate(&normalized) {
        return Some(build_narrative_event_language_certificate(
            &normalized,
            &actor,
            &event,
            object.as_deref(),
            negated,
        ));
    }

    if let Some(action) = parse_bare_instruction_candidate(&normalized) {
        return Some(build_instruction_language_certificate(
            &normalized,
            &action,
            false,
            true,
        ));
    }

    None
}

fn strip_leading_chat_markers(input: &str) -> &str {
    let mut text = input.trim();
    loop {
        let next = if let Some(rest) = text.strip_prefix("* ") {
            rest
        } else if let Some(rest) = text.strip_prefix("- ") {
            rest
        } else if let Some(rest) = text.strip_prefix("• ") {
            rest
        } else if let Some(rest) = text.strip_prefix("> ") {
            rest
        } else {
            break;
        };
        text = next.trim_start();
    }
    text
}

fn parse_narrative_state_candidate(input: &str) -> Option<(String, String, bool)> {
    for needle in [" is not ", " are not ", " was not ", " were not "] {
        if let Some((subject, state)) = input.split_once(needle) {
            let subject = subject.trim().to_string();
            let state = state.trim().to_string();
            if !subject.is_empty() && !state.is_empty() {
                return Some((subject, state, true));
            }
        }
    }

    for needle in [" is ", " are ", " was ", " were "] {
        if let Some((subject, state)) = input.split_once(needle) {
            let subject = subject.trim().to_string();
            let state = state.trim().to_string();
            if !subject.is_empty() && !state.is_empty() {
                return Some((subject, state, false));
            }
        }
    }

    None
}

fn parse_narrative_event_candidate(input: &str) -> Option<(String, String, Option<String>, bool)> {
    let tokens = input.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 2 {
        return None;
    }

    let negated = tokens.iter().any(|token| matches!(*token, "not" | "never"));
    let verb_index = tokens.iter().position(|token| {
        token.ends_with("ed") || matches!(*token, "caused" | "triggered" | "started" | "stopped" | "broke")
    })?;

    if verb_index == 0 {
        return None;
    }

    let actor = tokens[..verb_index].join(" ");
    let event = tokens[verb_index].to_string();
    let object_tokens = tokens[verb_index + 1..]
        .iter()
        .copied()
        .filter(|token| !matches!(*token, "not" | "never"))
        .collect::<Vec<_>>();
    let object = (!object_tokens.is_empty()).then(|| object_tokens.join(" "));
    Some((actor, event, object, negated))
}

fn parse_bare_instruction_candidate(input: &str) -> Option<String> {
    if input.contains(' ') && !input.contains(" is ") && !input.contains(" are ") {
        let first = input.split_whitespace().next()?;
        if matches!(first, "restart" | "start" | "stop" | "open" | "close" | "show" | "list" | "find" | "check") {
            return Some(input.to_string());
        }
    }
    None
}

fn build_instruction_plan(action_text: &str) -> (Vec<String>, Vec<SafeExecutableAction>) {
    let verb = action_text
        .split_whitespace()
        .next()
        .unwrap_or(action_text)
        .to_lowercase();

    match verb.as_str() {
        "restart" => (
            vec![
                "Check current service status first.".to_string(),
                "Perform a controlled restart only for the named target.".to_string(),
                "Verify health endpoint after restart.".to_string(),
            ],
            vec![
                SafeExecutableAction {
                    kind: "inspect".to_string(),
                    command: "systemctl status <service>".to_string(),
                    rationale: "Confirm current state before mutation.".to_string(),
                },
                SafeExecutableAction {
                    kind: "mutate".to_string(),
                    command: "systemctl restart <service>".to_string(),
                    rationale: "Controlled restart action.".to_string(),
                },
                SafeExecutableAction {
                    kind: "verify".to_string(),
                    command: "curl -fsS http://127.0.0.1:<port>/health".to_string(),
                    rationale: "Ensure service recovered correctly.".to_string(),
                },
            ],
        ),
        "start" => (
            vec![
                "Check whether target is already running.".to_string(),
                "Start the target process/service.".to_string(),
                "Verify readiness and logs.".to_string(),
            ],
            vec![
                SafeExecutableAction {
                    kind: "inspect".to_string(),
                    command: "systemctl is-active <service>".to_string(),
                    rationale: "Avoid duplicate starts.".to_string(),
                },
                SafeExecutableAction {
                    kind: "mutate".to_string(),
                    command: "systemctl start <service>".to_string(),
                    rationale: "Start target.".to_string(),
                },
                SafeExecutableAction {
                    kind: "verify".to_string(),
                    command: "journalctl -u <service> -n 50 --no-pager".to_string(),
                    rationale: "Validate startup behavior.".to_string(),
                },
            ],
        ),
        "stop" => (
            vec![
                "Inspect active sessions or dependents.".to_string(),
                "Stop target gracefully.".to_string(),
                "Confirm target is inactive.".to_string(),
            ],
            vec![
                SafeExecutableAction {
                    kind: "inspect".to_string(),
                    command: "systemctl status <service>".to_string(),
                    rationale: "Understand current use before stop.".to_string(),
                },
                SafeExecutableAction {
                    kind: "mutate".to_string(),
                    command: "systemctl stop <service>".to_string(),
                    rationale: "Graceful stop action.".to_string(),
                },
                SafeExecutableAction {
                    kind: "verify".to_string(),
                    command: "systemctl is-active <service>".to_string(),
                    rationale: "Ensure service is stopped.".to_string(),
                },
            ],
        ),
        _ => (
            vec![
                "Confirm exact target and scope.".to_string(),
                "Run a dry inspection command first.".to_string(),
                "Execute smallest safe action and verify outcome.".to_string(),
            ],
            vec![SafeExecutableAction {
                kind: "inspect".to_string(),
                command: "echo '<inspect target first>'".to_string(),
                rationale: "Default safe baseline when action family is unknown.".to_string(),
            }],
        ),
    }
}

fn build_instruction_language_certificate(
    normalized: &str,
    action_text: &str,
    negated: bool,
    ambiguous: bool,
) -> LanguageCertificate {
    let action_text = normalize_instruction_action_text(action_text);
    let (plan_steps, safe_actions) = build_instruction_plan(&action_text);
    let target = action_text
        .split_once(' ')
        .map(|(_, rest)| rest.trim().to_string())
        .filter(|value| !value.is_empty());
    let (execution_policy, dry_run_results) =
        simulate_instruction_actions(&action_text, target.as_deref(), &safe_actions);
    let best = SemanticFormV1 {
        version: 1,
        intent: SemanticIntent::InstructionRequest,
        atoms: vec![
            SemanticAtom {
                primitive: SemanticPrimitive::Query,
                role: Some("intent".to_string()),
                value: "instruction_request".to_string(),
            },
            SemanticAtom {
                primitive: SemanticPrimitive::Instruction,
                role: Some("action".to_string()),
                value: action_text.clone(),
            },
            SemanticAtom {
                primitive: SemanticPrimitive::Action,
                role: Some("verb".to_string()),
                value: action_text
                    .split_whitespace()
                    .next()
                    .unwrap_or(action_text.as_str())
                    .to_string(),
            },
            SemanticAtom {
                primitive: SemanticPrimitive::Negation,
                role: Some("present".to_string()),
                value: negated.to_string(),
            },
        ],
    };
    let alternatives = if ambiguous {
        vec![SemanticFormV1 {
            version: 1,
            intent: SemanticIntent::NarrativeEvent,
            atoms: vec![
                SemanticAtom {
                    primitive: SemanticPrimitive::Event,
                    role: Some("surface".to_string()),
                    value: action_text.clone(),
                },
                SemanticAtom {
                    primitive: SemanticPrimitive::Descriptor,
                    role: Some("interpretation".to_string()),
                    value: "bare imperative could also be a terse event label".to_string(),
                },
            ],
        }]
    } else {
        vec![]
    };

    LanguageCertificate {
        family: "language-instruction-request".to_string(),
        modality: "text".to_string(),
        parse: LanguageParseCertificate {
            normalized_input: normalized.to_string(),
            best,
            alternatives,
            rejections: vec![
                ParseRejection {
                    label: "confirm-relation".to_string(),
                    reason: "input does not include an explicit relation target pair".to_string(),
                },
                ParseRejection {
                    label: "describe-entity".to_string(),
                    reason: "input is action-oriented rather than classificatory".to_string(),
                },
            ],
            ambiguity_remaining: ambiguous,
            clarification_trigger: ambiguous.then(|| ClarificationTrigger {
                question: "Do you want me to treat this as an instruction to perform, or as a narrative event to interpret?".to_string(),
                options: vec!["instruction".to_string(), "narrative event".to_string()],
            }),
        },
        replay: LanguageCertificateReplay::InstructionRequest {
            action: action_text,
            target,
            negated,
            ambiguous,
            plan_steps,
            safe_actions,
            execution_policy,
            dry_run_results,
        },
    }
}

fn normalize_instruction_action_text(action_text: &str) -> String {
    let mut normalized = strip_leading_discourse_markers(action_text.trim())
        .trim_end_matches(['.', '?', '!'])
        .trim()
        .to_string();

    let mut tokens = normalized.split_whitespace().collect::<Vec<_>>();
    while matches!(tokens.last().copied(), Some("safely" | "carefully" | "securely" | "please")) {
        tokens.pop();
    }

    if !tokens.is_empty() {
        normalized = tokens.join(" ");
    }

    normalized
}

fn split_compound_query(input: &str) -> Vec<String> {
    let mut clauses = Vec::new();
    let mut current = String::new();

    for ch in input.chars() {
        if matches!(ch, '?' | '!' | '\n') {
            let clause = normalize_compound_clause(&current);
            if !clause.is_empty() {
                clauses.push(clause);
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }

    let trailing = normalize_compound_clause(&current);
    if !trailing.is_empty() {
        clauses.push(trailing);
    }

    if clauses.len() <= 1 {
        return clauses;
    }

    clauses
}

fn semantic_projection_for_clause(
    input: &str,
    answer: &str,
    certificate: Option<&ProofCertificate>,
) -> SemanticProjection {
    let normalized = normalize_compound_clause(input).to_ascii_lowercase();

    if let Some(certificate) = certificate {
        match certificate {
            ProofCertificate::Language(language) => {
                let (intent, mut meaning_tokens) = match &language.replay {
                    LanguageCertificateReplay::InstructionRequest { action, target, .. } => {
                        let mut tokens = vec![
                            "instruction".to_string(),
                            "plan".to_string(),
                            "safe-actions".to_string(),
                        ];
                        if let Some(verb) = action.split_whitespace().next() {
                            tokens.push(format!("verb:{verb}"));
                        }
                        if let Some(target) = target {
                            tokens.push(format!("target:{}", target.replace(' ', "_")));
                        }
                        ("instruction_request".to_string(), tokens)
                    }
                    LanguageCertificateReplay::ConfirmRelation { relation, .. } => (
                        "confirm_relation".to_string(),
                        vec![
                            "relation".to_string(),
                            format!("predicate:{relation}"),
                        ],
                    ),
                    LanguageCertificateReplay::DescribeEntity { subject, .. } => (
                        "describe_entity".to_string(),
                        vec![
                            "describe".to_string(),
                            format!("subject:{}", subject.replace(' ', "_")),
                        ],
                    ),
                    LanguageCertificateReplay::NarrativeState { subject, state, .. } => (
                        "narrative_state".to_string(),
                        vec![
                            "narrative".to_string(),
                            format!("subject:{}", subject.replace(' ', "_")),
                            format!("state:{}", state.replace(' ', "_")),
                        ],
                    ),
                    LanguageCertificateReplay::NarrativeEvent { actor, event, .. } => (
                        "narrative_event".to_string(),
                        vec![
                            "narrative".to_string(),
                            format!("actor:{}", actor.replace(' ', "_")),
                            format!("event:{event}"),
                        ],
                    ),
                };
                meaning_tokens.push(format!("family:{}", language.family));
                meaning_tokens.sort();
                meaning_tokens.dedup();
                return SemanticProjection { intent, meaning_tokens };
            }
            ProofCertificate::Math(math) => {
                let mut meaning_tokens = vec![
                    "math".to_string(),
                    "solve".to_string(),
                    format!("family:{}", math.family()),
                ];
                if normalized.contains("=") {
                    meaning_tokens.push("equation".to_string());
                }
                meaning_tokens.sort();
                meaning_tokens.dedup();
                return SemanticProjection {
                    intent: "solve_equation".to_string(),
                    meaning_tokens,
                };
            }
        }
    }

    if answer.contains("[CRYSTAL] [COMPUTE]") {
        return SemanticProjection {
            intent: "compute_expression".to_string(),
            meaning_tokens: vec!["compute".to_string(), "math".to_string()],
        };
    }

    if answer.starts_with("[NEEDS_INPUT]") {
        return SemanticProjection {
            intent: "needs_input".to_string(),
            meaning_tokens: vec!["clarification".to_string()],
        };
    }

    SemanticProjection {
        intent: "unknown".to_string(),
        meaning_tokens: vec!["unclassified".to_string()],
    }
}

fn normalize_compound_clause(input: &str) -> String {
    strip_leading_discourse_markers(strip_leading_chat_markers(input.trim()))
        .trim_end_matches(['.', '?', '!'])
        .trim()
        .to_string()
}

fn strip_leading_discourse_markers(input: &str) -> &str {
    let mut text = input.trim();
    loop {
        let lower = text.to_ascii_lowercase();
        let next = if let Some(rest) = lower
            .strip_prefix("also, ")
            .and_then(|_| text.get(6..))
        {
            rest
        } else if let Some(rest) = lower
            .strip_prefix("also ")
            .and_then(|_| text.get(5..))
        {
            rest
        } else if let Some(rest) = lower
            .strip_prefix("and also ")
            .and_then(|_| text.get(9..))
        {
            rest
        } else {
            break;
        };
        text = next.trim_start();
    }
    text
}

fn simulate_instruction_actions(
    _action_text: &str,
    target: Option<&str>,
    safe_actions: &[SafeExecutableAction],
) -> (ActionExecutionPolicy, Vec<SimulatedActionResult>) {
    let policy = ActionExecutionPolicy {
        mode: "dry-run".to_string(),
        allow_mutation: false,
        require_explicit_target: true,
        approved_kinds: vec![
            "inspect".to_string(),
            "verify".to_string(),
            "mutate".to_string(),
        ],
    };

    let target_ok = target.map(|value| !value.trim().is_empty()).unwrap_or(false);
    let results = safe_actions
        .iter()
        .map(|action| {
            let approved = policy.approved_kinds.iter().any(|kind| kind == &action.kind);
            let allowed = approved
                && (!policy.require_explicit_target || target_ok)
                && (policy.allow_mutation || action.kind != "mutate");
            let reason = if !approved {
                "kind not approved by policy".to_string()
            } else if policy.require_explicit_target && !target_ok {
                "missing explicit target".to_string()
            } else if !policy.allow_mutation && action.kind == "mutate" {
                "mutation disabled in dry-run mode".to_string()
            } else {
                "policy preconditions satisfied".to_string()
            };

            SimulatedActionResult {
                kind: action.kind.clone(),
                command: action.command.clone(),
                allowed,
                reason,
            }
        })
        .collect::<Vec<_>>();

    (policy, results)
}

fn build_narrative_state_language_certificate(
    normalized: &str,
    subject: &str,
    state: &str,
    negated: bool,
) -> LanguageCertificate {
    LanguageCertificate {
        family: "language-narrative-state".to_string(),
        modality: "text".to_string(),
        parse: LanguageParseCertificate {
            normalized_input: normalized.to_string(),
            best: SemanticFormV1 {
                version: 1,
                intent: SemanticIntent::NarrativeState,
                atoms: vec![
                    SemanticAtom {
                        primitive: SemanticPrimitive::Entity,
                        role: Some("subject".to_string()),
                        value: subject.to_string(),
                    },
                    SemanticAtom {
                        primitive: SemanticPrimitive::State,
                        role: Some("predicate".to_string()),
                        value: state.to_string(),
                    },
                    SemanticAtom {
                        primitive: SemanticPrimitive::Negation,
                        role: Some("present".to_string()),
                        value: negated.to_string(),
                    },
                ],
            },
            alternatives: vec![],
            rejections: vec![
                ParseRejection {
                    label: "instruction-request".to_string(),
                    reason: "input is stative rather than imperative".to_string(),
                },
                ParseRejection {
                    label: "confirm-relation".to_string(),
                    reason: "predicate is treated as a state phrase, not a class/relation target".to_string(),
                },
            ],
            ambiguity_remaining: false,
            clarification_trigger: None,
        },
        replay: LanguageCertificateReplay::NarrativeState {
            subject: subject.to_string(),
            state: state.to_string(),
            negated,
        },
    }
}

fn build_narrative_event_language_certificate(
    normalized: &str,
    actor: &str,
    event: &str,
    object: Option<&str>,
    negated: bool,
) -> LanguageCertificate {
    let mut atoms = vec![
        SemanticAtom {
            primitive: SemanticPrimitive::Entity,
            role: Some("actor".to_string()),
            value: actor.to_string(),
        },
        SemanticAtom {
            primitive: SemanticPrimitive::Event,
            role: Some("predicate".to_string()),
            value: event.to_string(),
        },
        SemanticAtom {
            primitive: SemanticPrimitive::Negation,
            role: Some("present".to_string()),
            value: negated.to_string(),
        },
    ];
    if let Some(object) = object {
        atoms.push(SemanticAtom {
            primitive: SemanticPrimitive::Entity,
            role: Some("object".to_string()),
            value: object.to_string(),
        });
    }

    LanguageCertificate {
        family: "language-narrative-event".to_string(),
        modality: "text".to_string(),
        parse: LanguageParseCertificate {
            normalized_input: normalized.to_string(),
            best: SemanticFormV1 {
                version: 1,
                intent: SemanticIntent::NarrativeEvent,
                atoms,
            },
            alternatives: vec![],
            rejections: vec![
                ParseRejection {
                    label: "instruction-request".to_string(),
                    reason: "surface contains an actor-event structure, not a direct imperative request".to_string(),
                },
                ParseRejection {
                    label: "describe-entity".to_string(),
                    reason: "input is eventive rather than classificatory".to_string(),
                },
            ],
            ambiguity_remaining: false,
            clarification_trigger: None,
        },
        replay: LanguageCertificateReplay::NarrativeEvent {
            actor: actor.to_string(),
            event: event.to_string(),
            object: object.map(|value| value.to_string()),
            negated,
        },
    }
}

fn replay_language_certificate(certificate: &LanguageCertificate) -> bool {
    match &certificate.replay {
        LanguageCertificateReplay::ConfirmRelation {
            relation,
            subject,
            object,
            proved,
            witness_path,
        } => {
            if certificate.family != "language-confirm-relation" || certificate.modality != "text" {
                return false;
            }
            let parse = &certificate.parse;
            if parse.ambiguity_remaining || !parse.alternatives.is_empty() {
                return false;
            }
            if parse.best.intent != SemanticIntent::ConfirmRelation || parse.best.version != 1 {
                return false;
            }
            let has_subject = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Entity)
                    && atom.role.as_deref() == Some("subject")
                    && atom.value == *subject
            });
            let has_object = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Entity)
                    && atom.role.as_deref() == Some("object")
                    && atom.value == *object
            });
            let has_relation = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Relation)
                    && atom.value == *relation
            });
            if !(has_subject && has_object && has_relation) {
                return false;
            }
            if *proved {
                witness_path.len() >= 2
                    && witness_path.first().map(|v| v == subject).unwrap_or(false)
                    && witness_path.last().map(|v| v == object).unwrap_or(false)
            } else {
                witness_path.is_empty()
            }
        }
        LanguageCertificateReplay::DescribeEntity {
            subject,
            direct_classes,
            properties,
            subtypes,
        } => {
            if certificate.family != "language-describe-entity" || certificate.modality != "text" {
                return false;
            }
            let parse = &certificate.parse;
            if parse.ambiguity_remaining || !parse.alternatives.is_empty() {
                return false;
            }
            if parse.best.intent != SemanticIntent::DescribeEntity || parse.best.version != 1 {
                return false;
            }
            let has_subject = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Entity)
                    && atom.role.as_deref() == Some("subject")
                    && atom.value == *subject
            });
            has_subject
                && direct_classes.iter().all(|value| !value.is_empty())
                && properties.iter().all(|value| !value.is_empty())
                && subtypes.iter().all(|value| !value.is_empty())
        }
        LanguageCertificateReplay::InstructionRequest {
            action,
            target,
            negated,
            ambiguous,
            plan_steps,
            safe_actions,
            execution_policy,
            dry_run_results,
        } => {
            if certificate.family != "language-instruction-request" || certificate.modality != "text" {
                return false;
            }
            let parse = &certificate.parse;
            if parse.best.intent != SemanticIntent::InstructionRequest || parse.best.version != 1 {
                return false;
            }
            let has_action = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Instruction)
                    && atom.role.as_deref() == Some("action")
                    && atom.value == *action
            });
            let has_negation = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Negation)
                    && atom.value == negated.to_string()
            });
            let target_ok = match target {
                Some(value) => action.contains(value),
                None => true,
            };
            let plan_ok = !plan_steps.is_empty() && !safe_actions.is_empty();
            let (expected_policy, expected_results) =
                simulate_instruction_actions(action, target.as_deref(), safe_actions);
            let simulation_ok = execution_policy == &expected_policy && dry_run_results == &expected_results;
            let ambiguity_ok = if *ambiguous {
                parse.ambiguity_remaining
                    && parse.clarification_trigger.is_some()
                    && !parse.alternatives.is_empty()
            } else {
                !parse.ambiguity_remaining && parse.clarification_trigger.is_none()
            };
            has_action && has_negation && target_ok && plan_ok && simulation_ok && ambiguity_ok
        }
        LanguageCertificateReplay::NarrativeState {
            subject,
            state,
            negated,
        } => {
            if certificate.family != "language-narrative-state" || certificate.modality != "text" {
                return false;
            }
            let parse = &certificate.parse;
            if parse.best.intent != SemanticIntent::NarrativeState || parse.best.version != 1 {
                return false;
            }
            let has_subject = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Entity)
                    && atom.role.as_deref() == Some("subject")
                    && atom.value == *subject
            });
            let has_state = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::State)
                    && atom.role.as_deref() == Some("predicate")
                    && atom.value == *state
            });
            let has_negation = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Negation)
                    && atom.value == negated.to_string()
            });
            has_subject && has_state && has_negation && !parse.ambiguity_remaining
        }
        LanguageCertificateReplay::NarrativeEvent {
            actor,
            event,
            object,
            negated,
        } => {
            if certificate.family != "language-narrative-event" || certificate.modality != "text" {
                return false;
            }
            let parse = &certificate.parse;
            if parse.best.intent != SemanticIntent::NarrativeEvent || parse.best.version != 1 {
                return false;
            }
            let has_actor = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Entity)
                    && atom.role.as_deref() == Some("actor")
                    && atom.value == *actor
            });
            let has_event = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Event)
                    && atom.role.as_deref() == Some("predicate")
                    && atom.value == *event
            });
            let object_ok = match object {
                Some(value) => parse.best.atoms.iter().any(|atom| {
                    matches!(atom.primitive, SemanticPrimitive::Entity)
                        && atom.role.as_deref() == Some("object")
                        && atom.value == *value
                }),
                None => true,
            };
            let has_negation = parse.best.atoms.iter().any(|atom| {
                matches!(atom.primitive, SemanticPrimitive::Negation)
                    && atom.value == negated.to_string()
            });
            has_actor && has_event && object_ok && has_negation && !parse.ambiguity_remaining
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

fn render_solve_equation(_equation: &str, solution: &EquationSolution) -> String {
    match solution {
        EquationSolution::LinearUnique { x, steps } => {
            let mut out = format!("[SOLVE] x = {}", format_compute_value(*x));
            if compute_latex_enabled() {
                for step in steps {
                    out.push_str(&format!("\n$$ {} $$", step));
                }
            }
            out
        }
        EquationSolution::QuadraticTwoRoots { x1, x2, steps } => {
            let mut out = format!(
                "[SOLVE] x1 = {}, x2 = {}",
                format_compute_value(*x1),
                format_compute_value(*x2)
            );
            if compute_latex_enabled() {
                for step in steps {
                    out.push_str(&format!("\n$$ {} $$", step));
                }
            }
            out
        }
        EquationSolution::QuadraticOneRoot { x, steps } => {
            let mut out = format!("[SOLVE] x = {}", format_compute_value(*x));
            if compute_latex_enabled() {
                for step in steps {
                    out.push_str(&format!("\n$$ {} $$", step));
                }
            }
            out
        }
        EquationSolution::SystemUnique { x, y, steps } => {
            let mut out = format!(
                "[SOLVE] x = {}, y = {}",
                format_compute_value(*x),
                format_compute_value(*y)
            );
            if compute_latex_enabled() {
                for step in steps {
                    out.push_str(&format!("\n$$ {} $$", step));
                }
            }
            out
        }
        EquationSolution::Textual { summary, steps } => {
            let mut out = format!("[SOLVE] {}", summary);
            if compute_latex_enabled() {
                for step in steps {
                    out.push_str(&format!("\n$$ {} $$", step));
                }
            }
            out
        }
        EquationSolution::TextualCertified {
            summary,
            steps,
            certificate,
        } => {
            debug_assert!(
                replay_solve_certificate(certificate),
                "invalid solve certificate for {}",
                certificate.family
            );
            let mut out = format!("[SOLVE] {}", summary);
            if compute_latex_enabled() {
                for step in steps {
                    out.push_str(&format!("\n$$ {} $$", step));
                }
            }
            out
        }
        EquationSolution::InfiniteSolutions { steps } => {
            let mut out = "[SOLVE] infinitely many solutions".to_string();
            if compute_latex_enabled() {
                for step in steps {
                    out.push_str(&format!("\n$$ {} $$", step));
                }
            }
            out
        }
        EquationSolution::NoSolution { steps } => {
            let mut out = "[SOLVE] no solution".to_string();
            if compute_latex_enabled() {
                for step in steps {
                    out.push_str(&format!("\n$$ {} $$", step));
                }
            }
            out
        }
        EquationSolution::NoRealRoots { steps } => {
            let mut out = "[SOLVE] no real roots".to_string();
            if compute_latex_enabled() {
                for step in steps {
                    out.push_str(&format!("\n$$ {} $$", step));
                }
            }
            out
        }
        EquationSolution::Unsupported => {
            "[SOLVE] unsupported equation form; use linear/quadratic in x or a 2-equation x/y system"
                .to_string()
        }
    }
}

#[derive(Debug, Clone)]
enum EquationSolution {
    LinearUnique { x: f64, steps: Vec<String> },
    QuadraticTwoRoots { x1: f64, x2: f64, steps: Vec<String> },
    QuadraticOneRoot { x: f64, steps: Vec<String> },
    SystemUnique { x: f64, y: f64, steps: Vec<String> },
    Textual { summary: String, steps: Vec<String> },
    TextualCertified {
        summary: String,
        steps: Vec<String>,
        certificate: SolveCertificate,
    },
    InfiniteSolutions { steps: Vec<String> },
    NoSolution { steps: Vec<String> },
    NoRealRoots { steps: Vec<String> },
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveCertificate {
    family: String,
    domain: IntervalSet,
    result_points: Vec<Rational>,
    result_intervals: IntervalSet,
    replay: SolveCertificateReplay,
}

impl SolveCertificate {
    pub fn family(&self) -> &str {
        &self.family
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum SolveCertificateReplay {
    RationalEquation {
        reduced: RationalExpr1D,
        excluded: Vec<Rational>,
    },
    RationalInequality {
        reduced: RationalExpr1D,
        op: InequalityOp,
        excluded: Vec<Rational>,
    },
    AbsRationalEquation {
        expr: RationalExpr1D,
        rhs: Rational,
        excluded: Vec<Rational>,
    },
    AbsRationalInequality {
        expr: RationalExpr1D,
        op: InequalityOp,
        rhs: Rational,
        excluded: Vec<Rational>,
    },
    RadicalEquation {
        radicand: QuadraticPoly,
        rhs: Rational,
    },
    RadicalInequality {
        radicand: QuadraticPoly,
        op: InequalityOp,
        rhs: Rational,
    },
}

fn solve_certificate_from_solution(solution: &EquationSolution) -> Option<ProofCertificate> {
    match solution {
        EquationSolution::TextualCertified { certificate, .. } => {
            Some(ProofCertificate::Math(certificate.clone()))
        }
        _ => None,
    }
}

pub fn verify_proof_certificate(certificate: &ProofCertificate) -> bool {
    match certificate {
        ProofCertificate::Math(certificate) => replay_solve_certificate(certificate),
        ProofCertificate::Language(certificate) => replay_language_certificate(certificate),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionExecutionDecision {
    pub ok: bool,
    pub executed: bool,
    pub requires_approval: bool,
    pub reason: String,
    pub family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_command: Option<String>,
}

pub fn evaluate_instruction_execution(
    certificate: &ProofCertificate,
    action_index: usize,
    approval_token: Option<&str>,
) -> InstructionExecutionDecision {
    let approval_secret = std::env::var("CSIF_EXEC_APPROVAL_TOKEN").ok();
    evaluate_instruction_execution_with_secret(
        certificate,
        action_index,
        approval_token,
        approval_secret.as_deref(),
    )
}

fn evaluate_instruction_execution_with_secret(
    certificate: &ProofCertificate,
    action_index: usize,
    approval_token: Option<&str>,
    approval_secret: Option<&str>,
) -> InstructionExecutionDecision {
    if !verify_proof_certificate(certificate) {
        return InstructionExecutionDecision {
            ok: false,
            executed: false,
            requires_approval: false,
            reason: "invalid or tampered proof certificate".to_string(),
            family: certificate.family().to_string(),
            action_kind: None,
            action_command: None,
        };
    }

    let ProofCertificate::Language(language) = certificate else {
        return InstructionExecutionDecision {
            ok: false,
            executed: false,
            requires_approval: false,
            reason: "execution requires a language instruction certificate".to_string(),
            family: certificate.family().to_string(),
            action_kind: None,
            action_command: None,
        };
    };

    let LanguageCertificateReplay::InstructionRequest {
        safe_actions,
        dry_run_results,
        ..
    } = &language.replay
    else {
        return InstructionExecutionDecision {
            ok: false,
            executed: false,
            requires_approval: false,
            reason: "certificate is not an instruction plan".to_string(),
            family: certificate.family().to_string(),
            action_kind: None,
            action_command: None,
        };
    };

    let Some(action) = safe_actions.get(action_index) else {
        return InstructionExecutionDecision {
            ok: false,
            executed: false,
            requires_approval: false,
            reason: "action index out of range".to_string(),
            family: certificate.family().to_string(),
            action_kind: None,
            action_command: None,
        };
    };

    let Some(sim) = dry_run_results.get(action_index) else {
        return InstructionExecutionDecision {
            ok: false,
            executed: false,
            requires_approval: false,
            reason: "dry-run result missing for selected action".to_string(),
            family: certificate.family().to_string(),
            action_kind: Some(action.kind.clone()),
            action_command: Some(action.command.clone()),
        };
    };

    if sim.kind != action.kind || sim.command != action.command {
        return InstructionExecutionDecision {
            ok: false,
            executed: false,
            requires_approval: false,
            reason: "dry-run/action mismatch for selected action".to_string(),
            family: certificate.family().to_string(),
            action_kind: Some(action.kind.clone()),
            action_command: Some(action.command.clone()),
        };
    }

    let is_mutating = action.kind == "mutate";
    let has_valid_approval = match (approval_secret, approval_token) {
        (Some(expected), Some(provided)) if !expected.is_empty() => provided == expected,
        _ => false,
    };

    if is_mutating && !has_valid_approval {
        return InstructionExecutionDecision {
            ok: false,
            executed: false,
            requires_approval: true,
            reason: "mutating actions require a valid approval token".to_string(),
            family: certificate.family().to_string(),
            action_kind: Some(action.kind.clone()),
            action_command: Some(action.command.clone()),
        };
    }

    // Allow controlled mutation only when token approval is valid and the sole block is dry-run mutate gate.
    if is_mutating
        && has_valid_approval
        && !sim.allowed
        && sim.reason == "mutation disabled in dry-run mode"
    {
        return InstructionExecutionDecision {
            ok: true,
            executed: true,
            requires_approval: false,
            reason: "mutation approved by token override".to_string(),
            family: certificate.family().to_string(),
            action_kind: Some(action.kind.clone()),
            action_command: Some(action.command.clone()),
        };
    }

    if !sim.allowed {
        return InstructionExecutionDecision {
            ok: false,
            executed: false,
            requires_approval: false,
            reason: format!("dry-run policy blocked action: {}", sim.reason),
            family: certificate.family().to_string(),
            action_kind: Some(action.kind.clone()),
            action_command: Some(action.command.clone()),
        };
    }

    InstructionExecutionDecision {
        ok: true,
        executed: true,
        requires_approval: false,
        reason: "action accepted by dry-run policy".to_string(),
        family: certificate.family().to_string(),
        action_kind: Some(action.kind.clone()),
        action_command: Some(action.command.clone()),
    }
}

fn singleton_interval_set(points: &[Rational]) -> IntervalSet {
    let uniq = sorted_unique_points(points.to_vec());
    let intervals = uniq
        .iter()
        .map(|p| Interval {
            lower: Some((*p, true)),
            upper: Some((*p, true)),
            is_empty: false,
        })
        .collect::<Vec<_>>();
    IntervalSet::from_intervals(intervals)
}

fn eval_quadratic(poly: QuadraticPoly, x: Rational) -> Option<Rational> {
    let x2 = x.mul(x)?;
    let ax2 = poly.a.mul(x2)?;
    let bx = poly.b.mul(x)?;
    ax2.add(bx)?.add(poly.c)
}

fn eval_rational_expr(expr: RationalExpr1D, x: Rational) -> Option<Rational> {
    let num = eval_quadratic(expr.num, x)?;
    let den = expr.den.0.mul(x)?.add(expr.den.1)?;
    if den.is_zero() {
        return None;
    }
    num.div(den)
}

fn replay_solve_certificate(cert: &SolveCertificate) -> bool {
    let in_domain = |x: Rational| cert.domain.contains_point(x);

    if cert
        .result_points
        .iter()
        .any(|p| !in_domain(*p) || !cert.result_intervals.contains_point(*p))
    {
        return false;
    }

    match &cert.replay {
        SolveCertificateReplay::RationalEquation { reduced, excluded } => {
            let recomputed_excluded = match denominator_exclusions(reduced.den) {
                Some(v) => sorted_unique_points(v),
                None => return false,
            };
            if sorted_unique_points(excluded.clone()) != recomputed_excluded {
                return false;
            }

            let roots = match polynomial_real_roots(reduced.num) {
                Some(v) => v,
                None => return false,
            };
            let mut valid = roots
                .into_iter()
                .filter(|r| !excluded.iter().any(|e| rational_cmp(*e, *r).is_eq()))
                .collect::<Vec<_>>();
            valid = sorted_unique_points(valid);
            if sorted_unique_points(cert.result_points.clone()) != valid {
                return false;
            }

            valid.iter().all(|r| {
                eval_rational_expr(*reduced, *r)
                    .map(|v| v.is_zero())
                    .unwrap_or(false)
            })
        }
        SolveCertificateReplay::RationalInequality { reduced, op, excluded } => {
            let solved = match rational_poly2_over_linear_intervals(*reduced, *op) {
                Some(v) => v,
                None => return false,
            };
            sorted_unique_points(excluded.clone()) == sorted_unique_points(solved.excluded)
                && cert.result_intervals == solved.intervals
        }
        SolveCertificateReplay::AbsRationalEquation {
            expr,
            rhs,
            excluded,
        } => {
            let mut roots = match solve_rational_equation_roots(*expr, *rhs) {
                Some(v) => v,
                None => return false,
            };
            if !rhs.is_zero() {
                let mut neg_roots = match solve_rational_equation_roots(*expr, rhs.negate()) {
                    Some(v) => v,
                    None => return false,
                };
                roots.append(&mut neg_roots);
            }
            roots = sorted_unique_points(roots);
            roots.retain(|r| !excluded.iter().any(|e| rational_cmp(*e, *r).is_eq()));

            if sorted_unique_points(cert.result_points.clone()) != roots {
                return false;
            }

            roots.iter().all(|r| {
                eval_rational_expr(*expr, *r)
                    .map(|v| v.abs() == *rhs)
                    .unwrap_or(false)
            })
        }
        SolveCertificateReplay::AbsRationalInequality {
            expr,
            op,
            rhs,
            excluded,
        } => {
            let recomputed = if rhs.num < 0 {
                match op {
                    InequalityOp::Lt | InequalityOp::Le => IntervalSet::empty(),
                    InequalityOp::Gt | InequalityOp::Ge => {
                        domain_interval_set_from_exclusions(excluded)
                    }
                }
            } else {
                let solved = match op {
                    InequalityOp::Le | InequalityOp::Lt => {
                        let low = match solve_rational_inequality_core(
                            *expr,
                            if matches!(op, InequalityOp::Le) {
                                InequalityOp::Ge
                            } else {
                                InequalityOp::Gt
                            },
                            rhs.negate(),
                        ) {
                            Some(v) => v,
                            None => return false,
                        };
                        let high = match solve_rational_inequality_core(*expr, *op, *rhs) {
                            Some(v) => v,
                            None => return false,
                        };
                        low.intervals.intersect(&high.intervals)
                    }
                    InequalityOp::Ge | InequalityOp::Gt => {
                        let low = match solve_rational_inequality_core(
                            *expr,
                            if matches!(op, InequalityOp::Ge) {
                                InequalityOp::Le
                            } else {
                                InequalityOp::Lt
                            },
                            rhs.negate(),
                        ) {
                            Some(v) => v,
                            None => return false,
                        };
                        let high = match solve_rational_inequality_core(*expr, *op, *rhs) {
                            Some(v) => v,
                            None => return false,
                        };
                        low.intervals.union(&high.intervals)
                    }
                };
                solved
            };
            recomputed == cert.result_intervals
        }
        SolveCertificateReplay::RadicalEquation { radicand, rhs } => {
            let domain = match poly_nonneg_domain(*radicand) {
                Some(v) => v,
                None => return false,
            };
            if domain != cert.domain {
                return false;
            }

            let points = if rhs.num < 0 {
                Vec::new()
            } else {
                let rhs_sq = match rhs.mul(*rhs) {
                    Some(v) => v,
                    None => return false,
                };
                let shifted = QuadraticPoly {
                    a: radicand.a,
                    b: radicand.b,
                    c: match radicand.c.sub(rhs_sq) {
                        Some(v) => v,
                        None => return false,
                    },
                };
                let candidates = match polynomial_real_roots(shifted) {
                    Some(v) => v,
                    None => return false,
                };
                sorted_unique_points(
                    candidates
                        .into_iter()
                        .filter(|x| eval_quadratic_sign(*radicand, *x).map(|s| s >= 0).unwrap_or(false))
                        .collect(),
                )
            };

            points == sorted_unique_points(cert.result_points.clone())
                && cert.result_intervals == singleton_interval_set(&points)
        }
        SolveCertificateReplay::RadicalInequality { radicand, op, rhs } => {
            let domain = match poly_nonneg_domain(*radicand) {
                Some(v) => v,
                None => return false,
            };
            if domain != cert.domain {
                return false;
            }

            let recomputed = if rhs.num < 0 {
                match op {
                    InequalityOp::Gt | InequalityOp::Ge => domain.clone(),
                    InequalityOp::Lt | InequalityOp::Le => IntervalSet::empty(),
                }
            } else {
                let rhs_sq = match rhs.mul(*rhs) {
                    Some(v) => v,
                    None => return false,
                };
                let shifted = QuadraticPoly {
                    a: radicand.a,
                    b: radicand.b,
                    c: match radicand.c.sub(rhs_sq) {
                        Some(v) => v,
                        None => return false,
                    },
                };
                let squared_set = match poly_sign_chart_intervals(shifted, *op) {
                    Some(v) => v,
                    None => return false,
                };
                domain.intersect(&squared_set)
            };

            recomputed == cert.result_intervals
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Rational {
    num: i128,
    den: i128,
}

impl Rational {
    fn new(num: i128, den: i128) -> Option<Self> {
        if den == 0 {
            return None;
        }
        let mut n = num;
        let mut d = den;
        if d < 0 {
            n = -n;
            d = -d;
        }
        if n == 0 {
            return Some(Self { num: 0, den: 1 });
        }
        let g = gcd_i128(n.abs(), d.abs());
        Some(Self {
            num: n / g,
            den: d / g,
        })
    }

    fn zero() -> Self {
        Self { num: 0, den: 1 }
    }

    fn is_zero(self) -> bool {
        self.num == 0
    }

    fn abs(self) -> Self {
        Self {
            num: self.num.abs(),
            den: self.den,
        }
    }

    fn negate(self) -> Self {
        Self {
            num: -self.num,
            den: self.den,
        }
    }

    fn add(self, other: Self) -> Option<Self> {
        Self::new(
            self.num.checked_mul(other.den)? + other.num.checked_mul(self.den)?,
            self.den.checked_mul(other.den)?,
        )
    }

    fn sub(self, other: Self) -> Option<Self> {
        self.add(other.negate())
    }

    fn mul(self, other: Self) -> Option<Self> {
        Self::new(
            self.num.checked_mul(other.num)?,
            self.den.checked_mul(other.den)?,
        )
    }

    fn div(self, other: Self) -> Option<Self> {
        if other.is_zero() {
            return None;
        }
        Self::new(
            self.num.checked_mul(other.den)?,
            self.den.checked_mul(other.num)?,
        )
    }

    fn from_decimal_str(text: &str) -> Option<Self> {
        let raw = text.trim();
        if raw.is_empty() {
            return None;
        }

        let mut sign = 1i128;
        let mut s = raw;
        if let Some(rest) = s.strip_prefix('+') {
            s = rest;
        } else if let Some(rest) = s.strip_prefix('-') {
            sign = -1;
            s = rest;
        }

        let mut parts = s.split('.');
        let int_part = parts.next()?;
        let frac_part = parts.next();
        if parts.next().is_some() {
            return None;
        }

        if let Some(frac) = frac_part {
            let int_val = if int_part.is_empty() {
                0
            } else {
                int_part.parse::<i128>().ok()?
            };
            let frac_digits = frac.len() as u32;
            let scale = 10i128.checked_pow(frac_digits)?;
            let frac_val = if frac.is_empty() {
                0
            } else {
                frac.parse::<i128>().ok()?
            };
            let num = int_val.checked_mul(scale)?.checked_add(frac_val)?;
            Self::new(sign.checked_mul(num)?, scale)
        } else {
            Self::new(sign.checked_mul(int_part.parse::<i128>().ok()?)?, 1)
        }
    }

    fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

fn gcd_i128(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    if a == 0 {
        1
    } else {
        a.abs()
    }
}

fn rational_to_latex(value: Rational) -> String {
    if value.den == 1 {
        value.num.to_string()
    } else if value.num < 0 {
        format!("-\\frac{{{}}}{{{}}}", value.num.abs(), value.den)
    } else {
        format!("\\frac{{{}}}{{{}}}", value.num, value.den)
    }
}

fn rational_to_plain(value: Rational) -> String {
    if value.den == 1 {
        value.num.to_string()
    } else {
        format!("{}/{}", value.num, value.den)
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
enum InequalityOp {
    Lt,
    Le,
    Gt,
    Ge,
}

impl InequalityOp {
    fn to_latex(self) -> &'static str {
        match self {
            InequalityOp::Lt => "<",
            InequalityOp::Le => "\\le",
            InequalityOp::Gt => ">",
            InequalityOp::Ge => "\\ge",
        }
    }

    fn flip(self) -> Self {
        match self {
            InequalityOp::Lt => InequalityOp::Gt,
            InequalityOp::Le => InequalityOp::Ge,
            InequalityOp::Gt => InequalityOp::Lt,
            InequalityOp::Ge => InequalityOp::Le,
        }
    }

    fn compare_to_zero(self, v: Rational) -> bool {
        match self {
            InequalityOp::Lt => v.num < 0,
            InequalityOp::Le => v.num <= 0,
            InequalityOp::Gt => v.num > 0,
            InequalityOp::Ge => v.num >= 0,
        }
    }
}

fn rational_cmp(left: Rational, right: Rational) -> std::cmp::Ordering {
    let lhs = left.num.checked_mul(right.den);
    let rhs = right.num.checked_mul(left.den);
    match (lhs, rhs) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => left
            .to_f64()
            .partial_cmp(&right.to_f64())
            .unwrap_or(std::cmp::Ordering::Equal),
    }
}

fn try_sqrt_rational(value: Rational) -> Option<Rational> {
    if value.num < 0 {
        return None;
    }
    let n = integer_sqrt_exact(value.num)?;
    let d = integer_sqrt_exact(value.den)?;
    Rational::new(n, d)
}

fn integer_sqrt_exact(value: i128) -> Option<i128> {
    if value < 0 {
        return None;
    }
    let root = (value as f64).sqrt().round() as i128;
    if root.checked_mul(root)? == value {
        Some(root)
    } else {
        None
    }
}

fn solve_equation(equation: &str) -> EquationSolution {
    if let Some(solution) = solve_linear_system_two_vars(equation) {
        return solution;
    }

    if let Some(solution) = solve_nxn_linear_system(equation) {
        return solution;
    }

    if let Some(solution) = solve_rational_inequality(equation) {
        return solution;
    }

    if let Some(solution) = solve_rational_expression_equation(equation) {
        return solution;
    }

    if let Some(solution) = solve_quadratic_inequality(equation) {
        return solution;
    }

    if let Some(solution) = solve_mixed_domain_inequality(equation) {
        return solution;
    }

    if let Some(solution) = solve_radical_equation(equation) {
        return solution;
    }

    if let Some(solution) = solve_radical_inequality(equation) {
        return solution;
    }

    if let Some(solution) = solve_absolute_value_inequality(equation) {
        return solution;
    }

    if let Some(solution) = solve_absolute_value_equation(equation) {
        return solution;
    }

    if let Some(solution) = solve_linear_inequality(equation) {
        return solution;
    }

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

    if let Some(((fa, fb), (ga, gb))) = parse_factorized_zero_form(left, right) {
        if fa.is_zero() || ga.is_zero() {
            return EquationSolution::Unsupported;
        }
        let Some(r1_rat) = fb.negate().div(fa) else {
            return EquationSolution::Unsupported;
        };
        let Some(r2_rat) = gb.negate().div(ga) else {
            return EquationSolution::Unsupported;
        };
        let r1 = r1_rat.to_f64();
        let r2 = r2_rat.to_f64();
        let (x1, x2) = if r1 <= r2 { (r1, r2) } else { (r2, r1) };
        let (xr1, xr2) = if r1 <= r2 {
            (r1_rat, r2_rat)
        } else {
            (r2_rat, r1_rat)
        };
        let steps = vec![
            equation_to_latex(equation),
            format!(
                "{} = 0 \\Rightarrow {} = 0 \\;\\text{{or}}\\; {} = 0",
                equation_to_latex(left),
                linear_factor_to_latex(fa, fb),
                linear_factor_to_latex(ga, gb)
            ),
            format!("x_1 = {}, x_2 = {}", rational_to_latex(xr1), rational_to_latex(xr2)),
        ];
        return EquationSolution::QuadraticTwoRoots { x1, x2, steps };
    }

    let Some((a1, b1, c1)) = parse_polynomial_up_to_quadratic(left) else {
        return EquationSolution::Unsupported;
    };
    let Some((a2, b2, c2)) = parse_polynomial_up_to_quadratic(right) else {
        return EquationSolution::Unsupported;
    };

    let Some(a) = a1.sub(a2) else {
        return EquationSolution::Unsupported;
    };
    let Some(b) = b1.sub(b2) else {
        return EquationSolution::Unsupported;
    };
    let Some(c) = c1.sub(c2) else {
        return EquationSolution::Unsupported;
    };

    let eps = 1e-12;

    if a.is_zero() {
        if b.is_zero() {
            if c.is_zero() {
                return EquationSolution::InfiniteSolutions {
                    steps: vec![equation_to_latex(equation), "0 = 0".to_string()],
                };
            }
            return EquationSolution::NoSolution {
                steps: vec![equation_to_latex(equation), format!("{} = 0", rational_to_latex(c))],
            };
        }

        let Some(x_ratio) = c.negate().div(b) else {
            return EquationSolution::Unsupported;
        };
        let x = x_ratio.to_f64();
        let steps = vec![
            equation_to_latex(equation),
            format!("{}x + {} = 0", rational_to_latex(b), rational_to_latex(c)),
            format!("x = {}", rational_to_latex(x_ratio)),
            format!("x = {}", format_compute_value(x)),
        ];
        return EquationSolution::LinearUnique { x, steps };
    }

    let Some(b_sq) = b.mul(b) else {
        return EquationSolution::Unsupported;
    };
    let Some(four_ac) = Rational::new(4, 1).and_then(|four| a.mul(c).and_then(|ac| four.mul(ac)))
    else {
        return EquationSolution::Unsupported;
    };
    let Some(disc_ratio) = b_sq.sub(four_ac) else {
        return EquationSolution::Unsupported;
    };

    let disc = disc_ratio.to_f64();
    if disc < -eps {
        return EquationSolution::NoRealRoots {
            steps: vec![
                equation_to_latex(equation),
                format!("\\Delta = b^2 - 4ac = {}", rational_to_latex(disc_ratio)),
            ],
        };
    }
    if disc.abs() <= eps {
        let Some(two_a) = Rational::new(2, 1).and_then(|two| two.mul(a)) else {
            return EquationSolution::Unsupported;
        };
        let Some(x_ratio) = b.negate().div(two_a) else {
            return EquationSolution::Unsupported;
        };
        let x = x_ratio.to_f64();
        let steps = vec![
            equation_to_latex(equation),
            format!(
                "x = \\frac{{-b}}{{2a}} = \\frac{{-\\left({}\\right)}}{{2\\cdot \\left({}\\right)}}",
                rational_to_latex(b),
                rational_to_latex(a)
            ),
            format!("x = {}", rational_to_latex(x_ratio)),
            format!("x = {}", format_compute_value(x)),
        ];
        return EquationSolution::QuadraticOneRoot { x, steps };
    }

    let a_f = a.to_f64();
    let b_f = b.to_f64();
    let c_f = c.to_f64();
    let sqrt_disc = disc.sqrt();
    let r1 = (-b_f - sqrt_disc) / (2.0 * a_f);
    let r2 = (-b_f + sqrt_disc) / (2.0 * a_f);
    let (x1, x2) = if r1 <= r2 { (r1, r2) } else { (r2, r1) };

    let exact_roots = try_sqrt_rational(disc_ratio)
        .and_then(|sqrt_ratio| {
            let two_a = Rational::new(2, 1)?.mul(a)?;
            let r_left = b.negate().sub(sqrt_ratio)?.div(two_a)?;
            let r_right = b.negate().add(sqrt_ratio)?.div(two_a)?;
            Some(if r_left.to_f64() <= r_right.to_f64() {
                (r_left, r_right)
            } else {
                (r_right, r_left)
            })
        });

    let roots_line = if let Some((rx1, rx2)) = exact_roots {
        format!("x_1 = {}, x_2 = {}", rational_to_latex(rx1), rational_to_latex(rx2))
    } else {
        format!("x_1 = {}, x_2 = {}", format_compute_value(x1), format_compute_value(x2))
    };
    let steps = vec![
        equation_to_latex(equation),
        format!(
            "\\Delta = b^2 - 4ac = \\left({}\\right)^2 - 4\\left({}\\right)\\left({}\\right) = {}",
            rational_to_latex(b),
            rational_to_latex(a),
            rational_to_latex(c),
            rational_to_latex(disc_ratio)
        ),
        "x = \\frac{-b \\pm \\sqrt{\\Delta}}{2a}".to_string(),
        roots_line,
        format!(
            "\\text{{check}}:\\; P(x_1) = {},\\; P(x_2) = {}",
            format_compute_value(a_f * x1 * x1 + b_f * x1 + c_f),
            format_compute_value(a_f * x2 * x2 + b_f * x2 + c_f)
        ),
    ];
    EquationSolution::QuadraticTwoRoots { x1, x2, steps }
}

fn solve_linear_system_two_vars(input: &str) -> Option<EquationSolution> {
    let normalized = input.replace('\n', ";");
    if !normalized.contains(';') {
        return None;
    }
    let equations: Vec<&str> = normalized
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if equations.len() != 2 {
        return None;
    }

    let (a1, b1, c1) = parse_linear_xy_equation(equations[0])?;
    let (a2, b2, c2) = parse_linear_xy_equation(equations[1])?;

    let det = a1.mul(b2)?.sub(a2.mul(b1)?)?;
    if det.is_zero() {
        let det_x = c1.mul(b2)?.sub(c2.mul(b1)?)?;
        let det_y = a1.mul(c2)?.sub(a2.mul(c1)?)?;
        if det_x.is_zero() && det_y.is_zero() {
            return Some(EquationSolution::InfiniteSolutions {
                steps: vec![
                    equation_to_latex(equations[0]),
                    equation_to_latex(equations[1]),
                    "\\Delta = 0".to_string(),
                ],
            });
        }
        return Some(EquationSolution::NoSolution {
            steps: vec![
                equation_to_latex(equations[0]),
                equation_to_latex(equations[1]),
                "\\Delta = 0".to_string(),
            ],
        });
    }

    let x_ratio = c1.mul(b2)?.sub(c2.mul(b1)?)?.div(det)?;
    let y_ratio = a1.mul(c2)?.sub(a2.mul(c1)?)?.div(det)?;
    let x = x_ratio.to_f64();
    let y = y_ratio.to_f64();
    Some(EquationSolution::SystemUnique {
        x,
        y,
        steps: vec![
            equation_to_latex(equations[0]),
            equation_to_latex(equations[1]),
            format!("\\Delta = {}", rational_to_latex(det)),
            format!("x = {}", rational_to_latex(x_ratio)),
            format!("y = {}", rational_to_latex(y_ratio)),
            format!("x \\approx {}", format_compute_value(x)),
            format!("y \\approx {}", format_compute_value(y)),
        ],
    })
}

fn solve_linear_inequality(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    if !contains_inequality_symbol(&compact) {
        return None;
    }

    let disjuncts = split_or_parts(&compact);
    let mut union_set = IntervalSet::empty();
    let mut steps = vec![equation_to_latex(equation)];

    for disjunct in disjuncts {
        let conjuncts = split_and_parts(disjunct);
        let mut current = IntervalSet::all_real();
        let mut local_constraints: Vec<String> = Vec::new();

        for conjunct in conjuncts {
            let simple_parts = expand_chained_inequality(conjunct)?;
            for simple in simple_parts {
                match solve_single_linear_inequality(&simple) {
                    Some(IneqSolve::AlwaysTrue { step }) => {
                        local_constraints.push(step);
                    }
                    Some(IneqSolve::AlwaysFalse { step }) => {
                        local_constraints.push(step);
                        current = IntervalSet::empty();
                    }
                    Some(IneqSolve::Constraint { intervals, step }) => {
                        local_constraints.push(step);
                        current = current.intersect(&intervals);
                    }
                    None => return Some(EquationSolution::Unsupported),
                }
            }
        }

        if !local_constraints.is_empty() {
            steps.push(local_constraints.join(" \\land "));
        }
        if !current.is_empty() {
            union_set = union_set.union(&current);
        }
    }

    if union_set.is_empty() {
        steps.push("\\varnothing".to_string());
        return Some(EquationSolution::Textual {
            summary: "no solution".to_string(),
            steps,
        });
    }

    let summary = format!("x in {}", union_set.to_plain_union());
    steps.push(format!("x \\in {}", union_set.to_latex_union()));
    Some(EquationSolution::Textual { summary, steps })
}

fn solve_quadratic_inequality(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    if !contains_inequality_symbol(&compact) {
        return None;
    }

    let (left, op, right) = split_inequality_once(&compact)?;
    let Some((a1, b1, c1)) = parse_polynomial_up_to_quadratic(left) else {
        return None;
    };
    let Some((a2, b2, c2)) = parse_polynomial_up_to_quadratic(right) else {
        return None;
    };

    let a = a1.sub(a2)?;
    let b = b1.sub(b2)?;
    let c = c1.sub(c2)?;
    let poly = quadratic_solution_intervals(a, b, c, op)?;
    Some(EquationSolution::Textual {
        summary: format!("x in {}", poly.to_plain_union()),
        steps: vec![equation_to_latex(equation), format!("x \\in {}", poly.to_latex_union())],
    })
}

fn solve_rational_inequality(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    if !contains_inequality_symbol(&compact) || !compact.contains('/') {
        return None;
    }

    if let Some(solution) = solve_domain_aware_rational_inequality(equation) {
        return Some(solution);
    }

    let (left, op, right) = split_inequality_once(&compact)?;
    if parse_number(right).is_none() {
        return None;
    }
    let right_zero = parse_number(right)?;
    if !right_zero.is_zero() {
        return None;
    }

    let frac = parse_linear_fraction(left)?;
    let intervals = rational_inequality_intervals(frac, op)?;
    Some(EquationSolution::Textual {
        summary: format!("x in {}", intervals.to_plain_union()),
        steps: vec![equation_to_latex(equation), format!("x \\in {}", intervals.to_latex_union())],
    })
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct QuadraticPoly {
    a: Rational,
    b: Rational,
    c: Rational,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct RationalExpr1D {
    num: QuadraticPoly,
    den: (Rational, Rational),
}

fn solve_rational_expression_equation(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    if !compact.contains('=') || !compact.contains('/') {
        return None;
    }
    let mut parts = compact.split('=');
    let left = parts.next()?;
    let right = parts.next()?;
    if parts.next().is_some() {
        return Some(EquationSolution::Unsupported);
    }

    let (expr, rhs_const) = if let Some(expr) = parse_rational_expr_1d(left) {
        let rhs = parse_number(right)?;
        (expr, rhs)
    } else if let Some(expr) = parse_rational_expr_1d(right) {
        let lhs = parse_number(left)?;
        (expr, lhs)
    } else {
        return None;
    };

    let reduced = shift_constant_from_ratio(expr, rhs_const)?;
    let excluded = denominator_exclusions(reduced.den)?;
    let roots = polynomial_real_roots(reduced.num)?;
    let mut valid_roots: Vec<Rational> = roots
        .into_iter()
        .filter(|r| !excluded.iter().any(|e| rational_cmp(*e, *r).is_eq()))
        .collect();
    valid_roots.sort_by(|l, r| rational_cmp(*l, *r));
    valid_roots.dedup_by(|l, r| rational_cmp(*l, *r).is_eq());

    let domain_set = domain_interval_set_from_exclusions(&excluded);
    let domain_plain = format_domain_plain(&excluded);
    let domain_latex = format_domain_latex(&excluded);
    let mut steps = vec![equation_to_latex(equation), domain_latex];

    let base_cert = SolveCertificate {
        family: "rational-equation".to_string(),
        domain: domain_set.clone(),
        result_points: Vec::new(),
        result_intervals: IntervalSet::empty(),
        replay: SolveCertificateReplay::RationalEquation {
            reduced,
            excluded: excluded.clone(),
        },
    };

    if valid_roots.is_empty() {
        steps.push("\\varnothing".to_string());
        return Some(EquationSolution::TextualCertified {
            summary: format!("no solution ({})", domain_plain),
            steps,
            certificate: base_cert,
        });
    }

    let summary = if valid_roots.len() == 1 {
        format!("x = {} ({})", rational_to_plain(valid_roots[0]), domain_plain)
    } else {
        format!(
            "x in {{{}}} ({})",
            valid_roots
                .iter()
                .map(|r| rational_to_plain(*r))
                .collect::<Vec<_>>()
                .join(", "),
            domain_plain
        )
    };
    steps.push(format!(
        "x \\in \\{{{}\\}}",
        valid_roots
            .iter()
            .map(|r| rational_to_latex(*r))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Some(EquationSolution::TextualCertified {
        summary,
        steps,
        certificate: SolveCertificate {
            family: "rational-equation".to_string(),
            domain: domain_set,
            result_points: valid_roots.clone(),
            result_intervals: singleton_interval_set(&valid_roots),
            replay: SolveCertificateReplay::RationalEquation { reduced, excluded },
        },
    })
}

fn solve_domain_aware_rational_inequality(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    if !contains_inequality_symbol(&compact) || !compact.contains('/') {
        return None;
    }

    let (mut left, mut op, mut right) = split_inequality_once(&compact)?;

    let left_ratio = left.contains('/');
    let right_ratio = right.contains('/');
    if !left_ratio && right_ratio {
        std::mem::swap(&mut left, &mut right);
        op = op.flip();
    }

    if parse_rational_expr_1d(left).is_none() {
        if parse_rational_expr_1d(right).is_none() {
            return None;
        }
        std::mem::swap(&mut left, &mut right);
        op = op.flip();
    }

    let expr = parse_rational_expr_1d(left)?;
    let rhs = parse_number(right)?;
    let reduced = shift_constant_from_ratio(expr, rhs)?;
    let solution = rational_poly2_over_linear_intervals(reduced, op)?;

    let domain_set = domain_interval_set_from_exclusions(&solution.excluded);
    let domain_plain = format_domain_plain(&solution.excluded);
    let domain_latex = format_domain_latex(&solution.excluded);
    let mut steps = vec![equation_to_latex(equation), domain_latex];
    if solution.intervals.is_empty() {
        steps.push("\\varnothing".to_string());
        return Some(EquationSolution::TextualCertified {
            summary: format!("no solution ({})", domain_plain),
            steps,
            certificate: SolveCertificate {
                family: "rational-inequality".to_string(),
                domain: domain_set,
                result_points: Vec::new(),
                result_intervals: IntervalSet::empty(),
                replay: SolveCertificateReplay::RationalInequality {
                    reduced,
                    op,
                    excluded: solution.excluded,
                },
            },
        });
    }

    steps.push(format!("x \\in {}", solution.intervals.to_latex_union()));
    Some(EquationSolution::TextualCertified {
        summary: format!("x in {} ({})", solution.intervals.to_plain_union(), domain_plain),
        steps,
        certificate: SolveCertificate {
            family: "rational-inequality".to_string(),
            domain: domain_set,
            result_points: Vec::new(),
            result_intervals: solution.intervals,
            replay: SolveCertificateReplay::RationalInequality {
                reduced,
                op,
                excluded: solution.excluded,
            },
        },
    })
}

fn solve_mixed_domain_inequality(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    if !contains_inequality_symbol(&compact) || !compact.contains('/') {
        return None;
    }
    if extract_abs_inner(&compact).is_some() {
        return None;
    }

    let disjuncts = split_or_parts(&compact);
    let mut union_set = IntervalSet::empty();
    let mut all_excluded: Vec<Rational> = Vec::new();
    let mut steps = vec![equation_to_latex(equation)];

    for disjunct in disjuncts {
        let conjuncts = split_and_parts(disjunct);
        let mut current = IntervalSet::all_real();

        for conjunct in conjuncts {
            let simple_parts = expand_chained_inequality(conjunct)?;
            for simple in simple_parts {
                let solved = solve_single_mixed_domain_inequality(&simple)?;
                all_excluded.extend(solved.excluded.iter().copied());
                current = current.intersect(&solved.intervals);
            }
        }

        if !current.is_empty() {
            union_set = union_set.union(&current);
        }
    }

    all_excluded = sorted_unique_points(all_excluded);
    let domain_plain = format_domain_plain(&all_excluded);
    let domain_latex = format_domain_latex(&all_excluded);
    steps.push(domain_latex);

    if union_set.is_empty() {
        steps.push("\\varnothing".to_string());
        return Some(EquationSolution::Textual {
            summary: format!("no solution ({})", domain_plain),
            steps,
        });
    }

    steps.push(format!("x \\in {}", union_set.to_latex_union()));
    Some(EquationSolution::Textual {
        summary: format!("x in {} ({})", union_set.to_plain_union(), domain_plain),
        steps,
    })
}

fn solve_single_mixed_domain_inequality(input: &str) -> Option<DomainIneqSolution> {
    let (mut left, mut op, mut right) = split_inequality_once(input)?;

    let left_ratio = left.contains('/');
    let right_ratio = right.contains('/');
    if !left_ratio && right_ratio {
        std::mem::swap(&mut left, &mut right);
        op = op.flip();
    }

    if parse_rational_expr_1d(left).is_none() {
        if parse_rational_expr_1d(right).is_none() {
            return None;
        }
        std::mem::swap(&mut left, &mut right);
        op = op.flip();
    }

    let expr = parse_rational_expr_1d(left)?;
    let rhs = parse_number(right)?;
    let reduced = shift_constant_from_ratio(expr, rhs)?;
    rational_poly2_over_linear_intervals(reduced, op)
}

struct DomainIneqSolution {
    intervals: IntervalSet,
    excluded: Vec<Rational>,
}

fn rational_poly2_over_linear_intervals(expr: RationalExpr1D, op: InequalityOp) -> Option<DomainIneqSolution> {
    let excluded = denominator_exclusions(expr.den)?;
    let mut critical = polynomial_real_roots(expr.num)?;
    critical.extend(excluded.iter().copied());
    critical = sorted_unique_points(critical);

    let mut out = Vec::new();
    for window in critical.windows(2) {
        let sample = sample_between(window[0], window[1]);
        if let Some(sign) = rational_poly_sign(expr, sample) {
            if sign_matches(sign, op) {
                out.push(Interval {
                    lower: Some((window[0], false)),
                    upper: Some((window[1], false)),
                    is_empty: false,
                });
            }
        }
    }

    if let Some(first) = critical.first().copied() {
        let sample = sample_left_of(first);
        if let Some(sign) = rational_poly_sign(expr, sample) {
            if sign_matches(sign, op) {
                out.push(Interval {
                    lower: None,
                    upper: Some((first, false)),
                    is_empty: false,
                });
            }
        }
    }

    if let Some(last) = critical.last().copied() {
        let sample = sample_right_of(last);
        if let Some(sign) = rational_poly_sign(expr, sample) {
            if sign_matches(sign, op) {
                out.push(Interval {
                    lower: Some((last, false)),
                    upper: None,
                    is_empty: false,
                });
            }
        }
    }

    if matches!(op, InequalityOp::Le | InequalityOp::Ge) {
        for r in polynomial_real_roots(expr.num)? {
            if excluded.iter().any(|e| rational_cmp(*e, r).is_eq()) {
                continue;
            }
            out.push(Interval {
                lower: Some((r, true)),
                upper: Some((r, true)),
                is_empty: false,
            });
        }
    }

    Some(DomainIneqSolution {
        intervals: IntervalSet::from_intervals(out),
        excluded,
    })
}

fn sign_matches(sign: i8, op: InequalityOp) -> bool {
    match op {
        InequalityOp::Gt => sign > 0,
        InequalityOp::Ge => sign >= 0,
        InequalityOp::Lt => sign < 0,
        InequalityOp::Le => sign <= 0,
    }
}

fn parse_rational_expr_1d(expr: &str) -> Option<RationalExpr1D> {
    let cleaned = strip_parens(expr.trim());
    let parts: Vec<&str> = cleaned.split('/').collect();
    if parts.len() > 2 {
        return None;
    }
    if parts.len() == 1 {
        let (a, b, c) = parse_polynomial_up_to_quadratic(parts[0])?;
        return Some(RationalExpr1D {
            num: QuadraticPoly { a, b, c },
            den: (Rational::zero(), Rational::new(1, 1)?),
        });
    }

    let (a, b, c) = parse_polynomial_up_to_quadratic(strip_parens(parts[0]))?;
    let den = parse_linear_x_expr(strip_parens(parts[1]))?;
    if den.0.is_zero() && den.1.is_zero() {
        return None;
    }
    Some(RationalExpr1D {
        num: QuadraticPoly { a, b, c },
        den,
    })
}

fn shift_constant_from_ratio(expr: RationalExpr1D, rhs: Rational) -> Option<RationalExpr1D> {
    let (da, db) = expr.den;
    let rhs_da = rhs.mul(da)?;
    let rhs_db = rhs.mul(db)?;
    let num = QuadraticPoly {
        a: expr.num.a,
        b: expr.num.b.sub(rhs_da)?,
        c: expr.num.c.sub(rhs_db)?,
    };
    Some(RationalExpr1D { num, den: expr.den })
}

fn denominator_exclusions(den: (Rational, Rational)) -> Option<Vec<Rational>> {
    let root = linear_root(den);
    if let Some(r) = root {
        Some(vec![r])
    } else {
        Some(vec![])
    }
}

fn format_domain_plain(excluded: &[Rational]) -> String {
    if excluded.is_empty() {
        return "domain: all real numbers".to_string();
    }
    format!(
        "domain: x != {}",
        excluded
            .iter()
            .map(|v| rational_to_plain(*v))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_domain_latex(excluded: &[Rational]) -> String {
    if excluded.is_empty() {
        return "\\text{domain}: x \\in (-\\infty, +\\infty)".to_string();
    }
    format!(
        "\\text{{domain}}: x \\ne {}",
        excluded
            .iter()
            .map(|v| rational_to_latex(*v))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn polynomial_real_roots(poly: QuadraticPoly) -> Option<Vec<Rational>> {
    if poly.a.is_zero() {
        if poly.b.is_zero() {
            return Some(vec![]);
        }
        return Some(vec![poly.c.negate().div(poly.b)?]);
    }
    let disc = poly.b.mul(poly.b)?.sub(Rational::new(4, 1)?.mul(poly.a.mul(poly.c)?)?)?;
    if disc.num < 0 {
        return Some(vec![]);
    }
    let sqrt = try_sqrt_rational(disc)?;
    let two_a = Rational::new(2, 1)?.mul(poly.a)?;
    let r1 = poly.b.negate().sub(sqrt)?.div(two_a)?;
    let r2 = poly.b.negate().add(sqrt)?.div(two_a)?;
    if rational_cmp(r1, r2).is_eq() {
        return Some(vec![r1]);
    }
    Some(vec![r1, r2])
}

fn evaluate_quadratic(poly: QuadraticPoly, x: Rational) -> Option<Rational> {
    poly.a
        .mul(x)?
        .mul(x)?
        .add(poly.b.mul(x)?)?
        .add(poly.c)
}

fn rational_poly_sign(expr: RationalExpr1D, x: Rational) -> Option<i8> {
    let n = evaluate_quadratic(expr.num, x)?;
    let d = evaluate_linear_at(expr.den, x)?;
    if d.is_zero() {
        return None;
    }
    let sn = n.num.signum() as i8;
    let sd = d.num.signum() as i8;
    Some(sn * sd)
}

fn quadratic_solution_intervals(a: Rational, b: Rational, c: Rational, op: InequalityOp) -> Option<IntervalSet> {
    let disc = b.mul(b)?.sub(Rational::new(4, 1)?.mul(a.mul(c)?)?)?;
    if disc.num < 0 {
        let sign = if matches!(op, InequalityOp::Gt | InequalityOp::Ge) {
            if a.num > 0 { IntervalSet::all_real() } else { IntervalSet::empty() }
        } else if a.num > 0 {
            IntervalSet::empty()
        } else {
            IntervalSet::all_real()
        };
        return Some(sign);
    }

    let sqrt = try_sqrt_rational(disc)?;
    let two_a = Rational::new(2, 1)?.mul(a)?;
    let r1 = b.negate().sub(sqrt)?.div(two_a)?;
    let r2 = b.negate().add(sqrt)?.div(two_a)?;
    let (lo, hi) = if rational_cmp(r1, r2).is_le() { (r1, r2) } else { (r2, r1) };

    let inclusive = matches!(op, InequalityOp::Le | InequalityOp::Ge);
    let left = IntervalSet::from_interval(Interval { lower: None, upper: Some((lo, inclusive)), is_empty: false });
    let middle = IntervalSet::from_interval(Interval { lower: Some((lo, inclusive)), upper: Some((hi, inclusive)), is_empty: false });
    let right = IntervalSet::from_interval(Interval { lower: Some((hi, inclusive)), upper: None, is_empty: false });

    let positive_outside = matches!(op, InequalityOp::Gt | InequalityOp::Ge);
    let intervals = if a.num > 0 {
        if positive_outside { left.union(&right) } else { middle }
    } else if positive_outside {
        middle
    } else {
        left.union(&right)
    };
    Some(intervals)
}

#[derive(Debug, Copy, Clone)]
struct LinearFraction {
    numerator: (Rational, Rational),
    denominator: (Rational, Rational),
}

fn parse_linear_fraction(expr: &str) -> Option<LinearFraction> {
    let cleaned = strip_parens(expr.trim());
    let parts: Vec<&str> = cleaned.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    Some(LinearFraction {
        numerator: parse_linear_x_expr(strip_parens(parts[0]))?,
        denominator: parse_linear_x_expr(strip_parens(parts[1]))?,
    })
}

fn rational_inequality_intervals(frac: LinearFraction, op: InequalityOp) -> Option<IntervalSet> {
    let num_root = linear_root(frac.numerator)?;
    let den_root = linear_root(frac.denominator)?;
    let critical = sorted_unique_points(vec![num_root, den_root]);
    let mut intervals = Vec::new();

    let sign_ok = |sign: i8| match op {
        InequalityOp::Gt => sign > 0,
        InequalityOp::Ge => sign >= 0,
        InequalityOp::Lt => sign < 0,
        InequalityOp::Le => sign <= 0,
    };

    for window in critical.windows(2) {
        let sample = sample_between(window[0], window[1]);
        if let Some(sign) = rational_fraction_sign(&frac, sample) {
            if sign_ok(sign) {
                intervals.push(Interval {
                    lower: Some((window[0], false)),
                    upper: Some((window[1], false)),
                    is_empty: false,
                });
            }
        }
    }

    if let Some(first) = critical.first().copied() {
        let sample = sample_left_of(first);
        if let Some(sign) = rational_fraction_sign(&frac, sample) {
            if sign_ok(sign) {
                intervals.push(Interval {
                    lower: None,
                    upper: Some((first, false)),
                    is_empty: false,
                });
            }
        }
    }

    if let Some(last) = critical.last().copied() {
        let sample = sample_right_of(last);
        if let Some(sign) = rational_fraction_sign(&frac, sample) {
            if sign_ok(sign) {
                intervals.push(Interval {
                    lower: Some((last, false)),
                    upper: None,
                    is_empty: false,
                });
            }
        }
    }

    if matches!(op, InequalityOp::Ge | InequalityOp::Le) {
        if num_root != den_root {
            let n_at_num = rational_fraction_zero_point_ok(&frac, num_root, den_root, op)?;
            if n_at_num {
                intervals.push(Interval { lower: Some((num_root, true)), upper: Some((num_root, true)), is_empty: false });
            }
        }
    }

    Some(IntervalSet::from_intervals(intervals))
}

fn rational_fraction_zero_point_ok(frac: &LinearFraction, num_root: Rational, den_root: Rational, op: InequalityOp) -> Option<bool> {
    if num_root == den_root {
        return Some(false);
    }
    let zero_ok = match op {
        InequalityOp::Gt | InequalityOp::Lt => false,
        InequalityOp::Ge | InequalityOp::Le => true,
    };
    let num = evaluate_linear_at(frac.numerator, num_root)?;
    let den = evaluate_linear_at(frac.denominator, num_root)?;
    Some(zero_ok && !den.is_zero() && num.is_zero())
}

fn linear_root(line: (Rational, Rational)) -> Option<Rational> {
    let (a, b) = line;
    if a.is_zero() {
        return None;
    }
    b.negate().div(a)
}

fn evaluate_linear_at(line: (Rational, Rational), x: Rational) -> Option<Rational> {
    let (a, b) = line;
    a.mul(x)?.add(b)
}

fn rational_fraction_sign(frac: &LinearFraction, x: Rational) -> Option<i8> {
    let n = evaluate_linear_at(frac.numerator, x)?;
    let d = evaluate_linear_at(frac.denominator, x)?;
    if d.is_zero() {
        return None;
    }
    let sn = n.num.signum() as i8;
    let sd = d.num.signum() as i8;
    Some(sn * sd)
}

fn sorted_unique_points(mut points: Vec<Rational>) -> Vec<Rational> {
    points.retain(|p| p.den != 0);
    points.sort_by(|a, b| rational_cmp(*a, *b));
    points.dedup_by(|a, b| rational_cmp(*a, *b).is_eq());
    points
}

fn sample_between(left: Rational, right: Rational) -> Rational {
    let Some(sum) = left.add(right) else {
        return left;
    };
    sum.div(Rational::new(2, 1).unwrap()).unwrap_or(left)
}

fn sample_left_of(point: Rational) -> Rational {
    point.sub(Rational::new(1, 1).unwrap()).unwrap_or(point)
}

fn sample_right_of(point: Rational) -> Rational {
    point.add(Rational::new(1, 1).unwrap()).unwrap_or(point)
}

fn solve_nxn_linear_system(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace('\n', ";");
    if !compact.contains(';') || !compact.contains('=') {
        return None;
    }
    let equations: Vec<&str> = compact
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if equations.len() < 3 {
        return None;
    }

    let vars = collect_linear_variables(&equations)?;
    if vars.len() < 3 {
        return None;
    }
    let parsed: Option<Vec<(Vec<Rational>, Rational)>> = equations.iter().map(|eq| parse_linear_n_system_equation(eq, &vars)).collect();
    let mut rows = parsed?;
    let vars = rows.first()?.0.len();
    if rows.len() != vars || vars == 0 {
        return Some(EquationSolution::Unsupported);
    }

    match gaussian_elimination(&mut rows)? {
        LinearSystemOutcome::Unique(solution) => {
            let mut steps = vec![equation_to_latex(equation), "\\text{Gaussian elimination}".to_string()];
            for (idx, value) in solution.iter().enumerate() {
                steps.push(format!("x_{} = {}", idx + 1, rational_to_latex(*value)));
            }
            let summary = solution.iter().enumerate().map(|(i, v)| format!("x{} = {}", i + 1, rational_to_plain(*v))).collect::<Vec<_>>().join(", ");
            Some(EquationSolution::Textual { summary, steps })
        }
        LinearSystemOutcome::Inconsistent => Some(EquationSolution::Textual {
            summary: "no solution".to_string(),
            steps: vec![equation_to_latex(equation), "\\text{Gaussian elimination}".to_string(), "\\text{inconsistent system}".to_string()],
        }),
        LinearSystemOutcome::Infinite => Some(EquationSolution::Textual {
            summary: "infinitely many solutions".to_string(),
            steps: vec![equation_to_latex(equation), "\\text{Gaussian elimination}".to_string(), "\\text{infinitely many solutions}".to_string()],
        }),
    }
}

fn collect_linear_variables(equations: &[&str]) -> Option<Vec<String>> {
    let mut vars = Vec::new();
    let mut seen = HashSet::new();
    for eq in equations {
        let compact = eq.replace(' ', "");
        let mut parts = compact.split('=');
        let left = parts.next()?;
        let right = parts.next()?;
        if parts.next().is_some() { return None; }
        for side in [left, right] {
            for term in tokenize_linear_terms(side) {
                if let Some(var) = term_variable_name(&term) {
                    if seen.insert(var.clone()) {
                        vars.push(var);
                    }
                }
            }
        }
    }
    Some(vars)
}

fn parse_linear_n_system_equation(equation: &str, vars: &[String]) -> Option<(Vec<Rational>, Rational)> {
    let compact = equation.replace(' ', "");
    let mut parts = compact.split('=');
    let left = parts.next()?;
    let right = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let (left_map, left_const) = parse_linear_expression_map(left)?;
    let (right_map, right_const) = parse_linear_expression_map(right)?;
    let mut coeffs = Vec::with_capacity(vars.len());
    for var in vars {
        let l = *left_map.get(var).unwrap_or(&Rational::zero());
        let r = *right_map.get(var).unwrap_or(&Rational::zero());
        coeffs.push(l.sub(r)?);
    }
    Some((coeffs, right_const.sub(left_const)?))
}

fn parse_linear_expression_map(expr: &str) -> Option<(HashMap<String, Rational>, Rational)> {
    let mut map = HashMap::new();
    let mut constant = Rational::zero();
    for term in tokenize_linear_terms(expr) {
        if let Some(var) = term_variable_name(&term) {
            let coeff = term_coeff(&term)?;
            let entry = map.entry(var).or_insert_with(Rational::zero);
            *entry = (*entry).add(coeff)?;
        } else {
            constant = constant.add(parse_number(&term)?)?;
        }
    }
    Some((map, constant))
}

fn tokenize_linear_terms(expr: &str) -> Vec<String> {
    let mut normalized = expr.replace('-', "+-");
    if let Some(rest) = normalized.strip_prefix("+-") {
        normalized = format!("-{}", rest);
    }
    normalized
        .split('+')
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.trim().to_string())
        .collect()
}

fn term_variable_name(term: &str) -> Option<String> {
    let idx = term.char_indices().find(|(_, c)| c.is_ascii_alphabetic())?.0;
    Some(term[idx..].to_string())
}

fn term_coeff(term: &str) -> Option<Rational> {
    let idx = term.char_indices().find(|(_, c)| c.is_ascii_alphabetic())?.0;
    parse_symbolic_coeff(&term[..idx])
}

fn strip_parens(expr: &str) -> &str {
    let mut text = expr.trim();
    loop {
        if !(text.starts_with('(') && text.ends_with(')')) {
            return text;
        }
        let mut depth = 0i32;
        let mut encloses_all = true;
        for (idx, ch) in text.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && idx + ch.len_utf8() != text.len() {
                        encloses_all = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if encloses_all {
            text = &text[1..text.len() - 1];
        } else {
            return text;
        }
    }
}

enum LinearSystemOutcome {
    Unique(Vec<Rational>),
    Inconsistent,
    Infinite,
}

fn gaussian_elimination(rows: &mut [(Vec<Rational>, Rational)]) -> Option<LinearSystemOutcome> {
    let n = rows.len();
    let mut pivot_count = 0;
    for col in 0..n {
        let Some(pivot) = (col..n).find(|&r| !rows[r].0.get(col).copied().unwrap_or(Rational::zero()).is_zero()) else {
            continue;
        };
        rows.swap(col, pivot);
        let pivot_val = rows[col].0[col];
        for c in col..n {
            rows[col].0[c] = rows[col].0[c].div(pivot_val)?;
        }
        rows[col].1 = rows[col].1.div(pivot_val)?;
        pivot_count += 1;
        for r in 0..n {
            if r == col { continue; }
            let factor = rows[r].0[col];
            if factor.is_zero() { continue; }
            for c in col..n {
                rows[r].0[c] = rows[r].0[c].sub(factor.mul(rows[col].0[c])?)?;
            }
            rows[r].1 = rows[r].1.sub(factor.mul(rows[col].1)?)?;
        }
    }
    for row in rows.iter() {
        let all_zero = row.0.iter().all(|c| c.is_zero());
        if all_zero && !row.1.is_zero() {
            return Some(LinearSystemOutcome::Inconsistent);
        }
    }
    if pivot_count < n {
        return Some(LinearSystemOutcome::Infinite);
    }
    Some(LinearSystemOutcome::Unique(rows.iter().map(|row| row.1).collect()))
}

fn solve_absolute_value_inequality(equation: &str) -> Option<EquationSolution> {
    if let Some(solution) = solve_absolute_value_rational_inequality(equation) {
        return Some(solution);
    }

    let compact = equation.replace(' ', "");
    if !contains_inequality_symbol(&compact) {
        return None;
    }

    let (mut left, mut op, mut right) = split_inequality_once(&compact)?;
    let inner = if let Some(inner) = extract_abs_inner(left) {
        inner
    } else if let Some(inner) = extract_abs_inner(right) {
        std::mem::swap(&mut left, &mut right);
        op = op.flip();
        inner
    } else {
        return None;
    };

    let rhs = match parse_number(right) {
        Some(v) => v,
        None => return Some(EquationSolution::Unsupported),
    };
    let (a, b) = match parse_linear_x_expr(inner) {
        Some(v) => v,
        None => return Some(EquationSolution::Unsupported),
    };

    let abs_expr = format!("\\left|{}\\right|", linear_factor_to_latex(a, b));
    let mut steps = vec![equation_to_latex(equation)];
    steps.push(format!(
        "{} {} {}",
        abs_expr,
        op.to_latex(),
        rational_to_latex(rhs)
    ));

    if rhs.num < 0 {
        let summary = match op {
            InequalityOp::Lt | InequalityOp::Le => "no solution".to_string(),
            InequalityOp::Gt | InequalityOp::Ge => "x in (-inf, +inf)".to_string(),
        };
        steps.push("\\left|\\cdot\\right| \\ge 0".to_string());
        return Some(EquationSolution::Textual { summary, steps });
    }

    let normalize_and_render = |intervals: IntervalSet, mut steps: Vec<String>| {
        if intervals.is_empty() {
            steps.push("\\varnothing".to_string());
            EquationSolution::Textual {
                summary: "no solution".to_string(),
                steps,
            }
        } else {
            let summary = format!("x in {}", intervals.to_plain_union());
            steps.push(format!("x \\in {}", intervals.to_latex_union()));
            EquationSolution::Textual { summary, steps }
        }
    };

    match op {
        InequalityOp::Le | InequalityOp::Lt => {
            let low_op = if matches!(op, InequalityOp::Le) {
                InequalityOp::Ge
            } else {
                InequalityOp::Gt
            };
            let high_op = op;

            steps.push(format!(
                "{} {} {} \\land {} {} {}",
                linear_factor_to_latex(a, b),
                low_op.to_latex(),
                rational_to_latex(rhs.negate()),
                linear_factor_to_latex(a, b),
                high_op.to_latex(),
                rational_to_latex(rhs)
            ));

            let left_solve = solve_linear_constraint(a, b, low_op, rhs.negate())?;
            let right_solve = solve_linear_constraint(a, b, high_op, rhs)?;

            let (left_intervals, left_step) = ineqsolve_to_interval_and_step(left_solve);
            let (right_intervals, right_step) = ineqsolve_to_interval_and_step(right_solve);
            steps.push(format!("{} \\land {}", left_step, right_step));

            let intervals = left_intervals.intersect(&right_intervals);
            Some(normalize_and_render(intervals, steps))
        }
        InequalityOp::Ge | InequalityOp::Gt => {
            let left_op = if matches!(op, InequalityOp::Ge) {
                InequalityOp::Le
            } else {
                InequalityOp::Lt
            };
            let right_op = op;

            steps.push(format!(
                "{} {} {} \\lor {} {} {}",
                linear_factor_to_latex(a, b),
                left_op.to_latex(),
                rational_to_latex(rhs.negate()),
                linear_factor_to_latex(a, b),
                right_op.to_latex(),
                rational_to_latex(rhs)
            ));

            let left_solve = solve_linear_constraint(a, b, left_op, rhs.negate())?;
            let right_solve = solve_linear_constraint(a, b, right_op, rhs)?;

            let (left_intervals, left_step) = ineqsolve_to_interval_and_step(left_solve);
            let (right_intervals, right_step) = ineqsolve_to_interval_and_step(right_solve);
            steps.push(format!("{} \\lor {}", left_step, right_step));

            Some(normalize_and_render(left_intervals.union(&right_intervals), steps))
        }
    }
}

fn ineqsolve_to_interval_and_step(solve: IneqSolve) -> (IntervalSet, String) {
    match solve {
        IneqSolve::Constraint { intervals, step } => (intervals, step),
        IneqSolve::AlwaysTrue { step } => (IntervalSet::all_real(), step),
        IneqSolve::AlwaysFalse { step } => (IntervalSet::empty(), step),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntervalSet {
    intervals: Vec<Interval>,
}

impl IntervalSet {
    fn empty() -> Self {
        Self { intervals: vec![] }
    }

    fn all_real() -> Self {
        Self {
            intervals: vec![Interval::all_real()],
        }
    }

    fn from_interval(interval: Interval) -> Self {
        Self::from_intervals(vec![interval])
    }

    fn from_intervals(intervals: Vec<Interval>) -> Self {
        Self {
            intervals: Self::normalize(intervals),
        }
    }

    fn is_empty(&self) -> bool {
        self.intervals.is_empty()
    }

    fn contains_point(&self, x: Rational) -> bool {
        self.intervals.iter().any(|i| i.contains_point(x))
    }

    fn union(&self, other: &Self) -> Self {
        let mut merged = self.intervals.clone();
        merged.extend(other.intervals.iter().copied());
        Self::from_intervals(merged)
    }

    #[allow(dead_code)]
    fn complement(&self) -> Self {
        if self.is_empty() {
            return Self::all_real();
        }

        let mut out = Vec::new();
        let mut cursor_lower: Option<(Rational, bool)> = None;

        for interval in &self.intervals {
            let gap_upper = interval.lower.map(|(v, inc)| (v, !inc));
            let gap = Interval {
                lower: cursor_lower,
                upper: gap_upper,
                is_empty: false,
            };
            let normalized = IntervalSet::from_interval(gap);
            out.extend(normalized.intervals);

            cursor_lower = interval.upper.map(|(v, inc)| (v, !inc));
        }

        let tail = Interval {
            lower: cursor_lower,
            upper: None,
            is_empty: false,
        };
        out.extend(IntervalSet::from_interval(tail).intervals);
        Self::from_intervals(out)
    }

    #[allow(dead_code)]
    fn difference(&self, other: &Self) -> Self {
        self.intersect(&other.complement())
    }

    fn intersect(&self, other: &Self) -> Self {
        if self.is_empty() || other.is_empty() {
            return Self::empty();
        }
        let mut out = Vec::new();
        for left in &self.intervals {
            for right in &other.intervals {
                let i = left.intersect(*right);
                if !i.is_empty {
                    out.push(i);
                }
            }
        }
        Self::from_intervals(out)
    }

    fn to_plain_union(&self) -> String {
        if self.intervals.is_empty() {
            return "empty".to_string();
        }
        if let Some((base, holes)) = self.hole_representation() {
            return format!(
                "{} \\ {{{}}}",
                base.to_plain(),
                holes
                    .into_iter()
                    .map(rational_to_plain)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        self.intervals
            .iter()
            .map(Interval::to_plain)
            .collect::<Vec<_>>()
            .join(" U ")
    }

    fn to_latex_union(&self) -> String {
        if self.intervals.is_empty() {
            return "\\varnothing".to_string();
        }
        if let Some((base, holes)) = self.hole_representation() {
            return format!(
                "{} \\setminus \\{{{}\\}}",
                base.to_latex(),
                holes
                    .into_iter()
                    .map(rational_to_latex)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        self.intervals
            .iter()
            .map(Interval::to_latex)
            .collect::<Vec<_>>()
            .join(" \\cup ")
    }

    fn from_constraint(op: InequalityOp, bound: Rational) -> Self {
        let interval = match op {
            InequalityOp::Lt => Interval {
                lower: None,
                upper: Some((bound, false)),
                is_empty: false,
            },
            InequalityOp::Le => Interval {
                lower: None,
                upper: Some((bound, true)),
                is_empty: false,
            },
            InequalityOp::Gt => Interval {
                lower: Some((bound, false)),
                upper: None,
                is_empty: false,
            },
            InequalityOp::Ge => Interval {
                lower: Some((bound, true)),
                upper: None,
                is_empty: false,
            },
        };
        Self::from_interval(interval)
    }

    fn normalize(mut intervals: Vec<Interval>) -> Vec<Interval> {
        intervals.retain(|i| !i.is_empty);
        if intervals.is_empty() {
            return intervals;
        }
        intervals.sort_by(Self::lower_cmp);

        let mut merged: Vec<Interval> = Vec::new();
        for interval in intervals {
            if let Some(last) = merged.last_mut() {
                if last.touches_or_overlaps(interval) {
                    *last = last.union_with(interval);
                } else {
                    merged.push(interval);
                }
            } else {
                merged.push(interval);
            }
        }
        merged
    }

    fn lower_cmp(a: &Interval, b: &Interval) -> std::cmp::Ordering {
        match (a.lower, b.lower) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some((av, ai)), Some((bv, bi))) => match rational_cmp(av, bv) {
                std::cmp::Ordering::Equal => bi.cmp(&ai),
                other => other,
            },
        }
    }

    fn hole_representation(&self) -> Option<(Interval, Vec<Rational>)> {
        if self.intervals.len() < 2 {
            return None;
        }

        let first = *self.intervals.first()?;
        let last = *self.intervals.last()?;
        let base = Interval {
            lower: first.lower,
            upper: last.upper,
            is_empty: false,
        };

        let mut holes = Vec::new();
        for pair in self.intervals.windows(2) {
            let left = pair[0];
            let right = pair[1];
            let (Some((lu, li)), Some((rl, ri))) = (left.upper, right.lower) else {
                return None;
            };
            if !rational_cmp(lu, rl).is_eq() || li || ri {
                return None;
            }
            holes.push(lu);
        }
        if holes.is_empty() {
            None
        } else {
            Some((base, holes))
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interval {
    lower: Option<(Rational, bool)>,
    upper: Option<(Rational, bool)>,
    is_empty: bool,
}

impl Interval {
    fn all_real() -> Self {
        Self {
            lower: None,
            upper: None,
            is_empty: false,
        }
    }

    fn empty() -> Self {
        Self {
            lower: None,
            upper: None,
            is_empty: true,
        }
    }

    fn contains_point(&self, x: Rational) -> bool {
        if self.is_empty {
            return false;
        }

        if let Some((lv, inclusive)) = self.lower {
            match rational_cmp(x, lv) {
                std::cmp::Ordering::Less => return false,
                std::cmp::Ordering::Equal if !inclusive => return false,
                _ => {}
            }
        }

        if let Some((uv, inclusive)) = self.upper {
            match rational_cmp(x, uv) {
                std::cmp::Ordering::Greater => return false,
                std::cmp::Ordering::Equal if !inclusive => return false,
                _ => {}
            }
        }

        true
    }

    fn intersect(&self, right: Self) -> Self {
        let left = *self;
        if left.is_empty || right.is_empty {
            return Interval::empty();
        }

        let lower = match (left.lower, right.lower) {
            (Some(l), Some(r)) => match rational_cmp(l.0, r.0) {
                std::cmp::Ordering::Greater => Some(l),
                std::cmp::Ordering::Less => Some(r),
                std::cmp::Ordering::Equal => Some((l.0, l.1 && r.1)),
            },
            (Some(l), None) => Some(l),
            (None, Some(r)) => Some(r),
            (None, None) => None,
        };

        let upper = match (left.upper, right.upper) {
            (Some(l), Some(r)) => match rational_cmp(l.0, r.0) {
                std::cmp::Ordering::Greater => Some(r),
                std::cmp::Ordering::Less => Some(l),
                std::cmp::Ordering::Equal => Some((l.0, l.1 && r.1)),
            },
            (Some(l), None) => Some(l),
            (None, Some(r)) => Some(r),
            (None, None) => None,
        };

        if let (Some((lv, li)), Some((uv, ui))) = (lower, upper) {
            match rational_cmp(lv, uv) {
                std::cmp::Ordering::Greater => return Interval::empty(),
                std::cmp::Ordering::Equal if !(li && ui) => return Interval::empty(),
                _ => {}
            }
        }

        Interval {
            lower,
            upper,
            is_empty: false,
        }
    }

    fn touches_or_overlaps(&self, right: Self) -> bool {
        let left = *self;
        match (left.upper, right.lower) {
            (None, _) => true,
            (_, None) => true,
            (Some((au, ai)), Some((bl, bi))) => match rational_cmp(au, bl) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Less => false,
                std::cmp::Ordering::Equal => ai || bi,
            },
        }
    }

    fn union_with(&self, right: Self) -> Self {
        let left = *self;
        let lower = match (left.lower, right.lower) {
            (None, _) | (_, None) => None,
            (Some(l), Some(r)) => match rational_cmp(l.0, r.0) {
                std::cmp::Ordering::Less => Some(l),
                std::cmp::Ordering::Greater => Some(r),
                std::cmp::Ordering::Equal => Some((l.0, l.1 || r.1)),
            },
        };

        let upper = match (left.upper, right.upper) {
            (None, _) | (_, None) => None,
            (Some(l), Some(r)) => match rational_cmp(l.0, r.0) {
                std::cmp::Ordering::Less => Some(r),
                std::cmp::Ordering::Greater => Some(l),
                std::cmp::Ordering::Equal => Some((l.0, l.1 || r.1)),
            },
        };

        Interval {
            lower,
            upper,
            is_empty: false,
        }
    }

    fn to_plain(&self) -> String {
        if let (Some((lv, true)), Some((uv, true))) = (self.lower, self.upper) {
            if rational_cmp(lv, uv).is_eq() {
                return format!("{{{}}}", rational_to_plain(lv));
            }
        }
        let left_bracket = match self.lower {
            Some((_, true)) => "[",
            _ => "(",
        };
        let right_bracket = match self.upper {
            Some((_, true)) => "]",
            _ => ")",
        };
        let lower = self
            .lower
            .map(|(v, _)| rational_to_plain(v))
            .unwrap_or_else(|| "-inf".to_string());
        let upper = self
            .upper
            .map(|(v, _)| rational_to_plain(v))
            .unwrap_or_else(|| "+inf".to_string());
        format!("{}{}, {}{}", left_bracket, lower, upper, right_bracket)
    }

    fn to_latex(&self) -> String {
        if let (Some((lv, true)), Some((uv, true))) = (self.lower, self.upper) {
            if rational_cmp(lv, uv).is_eq() {
                return format!("\\{{{}\\}}", rational_to_latex(lv));
            }
        }
        let left_bracket = match self.lower {
            Some((_, true)) => "[",
            _ => "(",
        };
        let right_bracket = match self.upper {
            Some((_, true)) => "]",
            _ => ")",
        };
        let lower = self
            .lower
            .map(|(v, _)| rational_to_latex(v))
            .unwrap_or_else(|| "-\\infty".to_string());
        let upper = self
            .upper
            .map(|(v, _)| rational_to_latex(v))
            .unwrap_or_else(|| "+\\infty".to_string());
        format!("{}{}, {}{}", left_bracket, lower, upper, right_bracket)
    }

}

enum IneqSolve {
    Constraint { intervals: IntervalSet, step: String },
    AlwaysTrue { step: String },
    AlwaysFalse { step: String },
}

fn solve_single_linear_inequality(input: &str) -> Option<IneqSolve> {
    let (left, op, right) = split_inequality_once(input)?;

    let (la, lb) = parse_linear_x_expr(left)?;
    let (ra, rb) = parse_linear_x_expr(right)?;
    let a = la.sub(ra)?;
    let b = lb.sub(rb)?;

    solve_linear_constraint(a, b, op, Rational::zero())
}

fn solve_linear_constraint(a: Rational, b: Rational, op: InequalityOp, rhs: Rational) -> Option<IneqSolve> {
    let c = b.sub(rhs)?;

    if a.is_zero() {
        let step = format!("{} {} 0", rational_to_latex(c), op.to_latex());
        if op.compare_to_zero(c) {
            return Some(IneqSolve::AlwaysTrue { step });
        }
        return Some(IneqSolve::AlwaysFalse { step });
    }

    let mut solved_op = op;
    if a.num < 0 {
        solved_op = solved_op.flip();
    }
    let bound = c.negate().div(a)?;
    let step = format!(
        "{} {} 0 \\Rightarrow x {} {}",
        linear_factor_to_latex(a, c),
        op.to_latex(),
        solved_op.to_latex(),
        rational_to_latex(bound)
    );

    Some(IneqSolve::Constraint {
        intervals: IntervalSet::from_constraint(solved_op, bound),
        step,
    })
}

fn contains_inequality_symbol(input: &str) -> bool {
    input.contains('<') || input.contains('>') || input.contains('≤') || input.contains('≥')
}

fn split_or_parts(input: &str) -> Vec<&str> {
    input
        .split("or")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

fn split_and_parts(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for part in input.split(';') {
        for inner in part.split("and") {
            let t = inner.trim();
            if !t.is_empty() {
                out.push(t);
            }
        }
    }
    out
}

fn expand_chained_inequality(input: &str) -> Option<Vec<String>> {
    let tokens = tokenize_inequality(input)?;
    if tokens.ops.len() == 1 {
        return Some(vec![input.to_string()]);
    }
    if tokens.ops.len() == 2 && tokens.parts.len() == 3 {
        return Some(vec![
            format!("{}{}{}", tokens.parts[0], tokens.ops[0].0, tokens.parts[1]),
            format!("{}{}{}", tokens.parts[1], tokens.ops[1].0, tokens.parts[2]),
        ]);
    }
    None
}

struct IneqTokens {
    parts: Vec<String>,
    ops: Vec<(&'static str, InequalityOp)>,
}

fn tokenize_inequality(input: &str) -> Option<IneqTokens> {
    let mut parts = Vec::new();
    let mut ops = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    let mut last = 0usize;

    while i < bytes.len() {
        let slice = &input[i..];
        let found = if slice.starts_with("<=") {
            Some(("<=", InequalityOp::Le, 2usize))
        } else if slice.starts_with(">=") {
            Some((">=", InequalityOp::Ge, 2usize))
        } else if slice.starts_with("≤") {
            Some(("≤", InequalityOp::Le, "≤".len()))
        } else if slice.starts_with("≥") {
            Some(("≥", InequalityOp::Ge, "≥".len()))
        } else if slice.starts_with('<') {
            Some(("<", InequalityOp::Lt, 1usize))
        } else if slice.starts_with('>') {
            Some((">", InequalityOp::Gt, 1usize))
        } else {
            None
        };

        if let Some((symbol, op, width)) = found {
            parts.push(input[last..i].to_string());
            ops.push((symbol, op));
            i += width;
            last = i;
        } else {
            i += 1;
        }
    }

    if ops.is_empty() {
        return None;
    }
    parts.push(input[last..].to_string());
    Some(IneqTokens { parts, ops })
}

fn split_inequality_once(input: &str) -> Option<(&str, InequalityOp, &str)> {
    for (needle, op) in [
        ("<=", InequalityOp::Le),
        (">=", InequalityOp::Ge),
        ("≤", InequalityOp::Le),
        ("≥", InequalityOp::Ge),
        ("<", InequalityOp::Lt),
        (">", InequalityOp::Gt),
    ] {
        if let Some(idx) = input.find(needle) {
            let left = &input[..idx];
            let right = &input[idx + needle.len()..];
            if left.is_empty() || right.is_empty() {
                return None;
            }
            return Some((left, op, right));
        }
    }
    None
}

fn solve_absolute_value_equation(equation: &str) -> Option<EquationSolution> {
    if let Some(solution) = solve_absolute_value_rational_equation(equation) {
        return Some(solution);
    }

    let compact = equation.replace(' ', "");
    let mut parts = compact.split('=');
    let left = parts.next()?;
    let right = parts.next()?;
    if parts.next().is_some() {
        return Some(EquationSolution::Unsupported);
    }

    let (inner, rhs) = if let Some(inner) = extract_abs_inner(left) {
        (inner, parse_number(right)?)
    } else if let Some(inner) = extract_abs_inner(right) {
        (inner, parse_number(left)?)
    } else {
        return None;
    };

    let (a, b) = match parse_linear_x_expr(inner) {
        Some(v) => v,
        None => return Some(EquationSolution::Unsupported),
    };

    let lhs_abs = format!("\\left|{}\\right|", linear_factor_to_latex(a, b));
    if rhs.num < 0 {
        return Some(EquationSolution::Textual {
            summary: "no solution".to_string(),
            steps: vec![
                equation_to_latex(equation),
                format!("{} = {}", lhs_abs, rational_to_latex(rhs)),
                "\\left|\\cdot\\right| \\ge 0".to_string(),
            ],
        });
    }

    if a.is_zero() {
        let const_abs = b.abs();
        let summary = if const_abs == rhs {
            "all real numbers".to_string()
        } else {
            "no solution".to_string()
        };
        return Some(EquationSolution::Textual {
            summary,
            steps: vec![
                equation_to_latex(equation),
                format!("{} = {}", lhs_abs, rational_to_latex(rhs)),
                format!(
                    "{} = {}",
                    rational_to_latex(const_abs),
                    rational_to_latex(rhs)
                ),
            ],
        });
    }

    let x1 = rhs.sub(b)?.div(a)?;
    let x2 = rhs.negate().sub(b)?.div(a)?;
    if x1 == x2 {
        return Some(EquationSolution::Textual {
            summary: format!("x = {}", rational_to_plain(x1)),
            steps: vec![
                equation_to_latex(equation),
                format!("{} = {}", lhs_abs, rational_to_latex(rhs)),
                format!("{} = 0", linear_factor_to_latex(a, b)),
                format!("x = {}", rational_to_latex(x1)),
            ],
        });
    }

    let (s1, s2) = if rational_cmp(x1, x2).is_le() {
        (x1, x2)
    } else {
        (x2, x1)
    };
    Some(EquationSolution::Textual {
        summary: format!("x1 = {}, x2 = {}", rational_to_plain(s1), rational_to_plain(s2)),
        steps: vec![
            equation_to_latex(equation),
            format!("{} = {}", lhs_abs, rational_to_latex(rhs)),
            format!(
                "{} = {} \\;\\text{{or}}\\; {} = {}",
                linear_factor_to_latex(a, b),
                rational_to_latex(rhs),
                linear_factor_to_latex(a, b),
                rational_to_latex(rhs.negate())
            ),
            format!("x_1 = {}, x_2 = {}", rational_to_latex(s1), rational_to_latex(s2)),
        ],
    })
}

fn solve_absolute_value_rational_equation(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    let mut parts = compact.split('=');
    let left = parts.next()?;
    let right = parts.next()?;
    if parts.next().is_some() {
        return Some(EquationSolution::Unsupported);
    }

    let (inner, rhs) = if let Some(inner) = extract_abs_inner(left) {
        (inner, parse_number(right)?)
    } else if let Some(inner) = extract_abs_inner(right) {
        (inner, parse_number(left)?)
    } else {
        return None;
    };

    if !inner.contains('/') {
        return None;
    }

    let expr = parse_rational_expr_1d(inner)?;
    let excluded = denominator_exclusions(expr.den)?;
    let domain_set = domain_interval_set_from_exclusions(&excluded);
    let domain_plain = format_domain_plain(&excluded);
    let domain_latex = format_domain_latex(&excluded);
    let mut steps = vec![equation_to_latex(equation), domain_latex];

    if rhs.num < 0 {
        steps.push("\\varnothing".to_string());
        return Some(EquationSolution::TextualCertified {
            summary: format!("no solution ({})", domain_plain),
            steps,
            certificate: SolveCertificate {
                family: "abs-rational-equation".to_string(),
                domain: domain_set,
                result_points: Vec::new(),
                result_intervals: IntervalSet::empty(),
                replay: SolveCertificateReplay::AbsRationalEquation {
                    expr,
                    rhs,
                    excluded,
                },
            },
        });
    }

    let mut roots = Vec::new();
    roots.extend(solve_rational_equation_roots(expr, rhs)?);
    if !rhs.is_zero() {
        roots.extend(solve_rational_equation_roots(expr, rhs.negate())?);
    }
    roots = sorted_unique_points(roots);
    roots.retain(|r| !excluded.iter().any(|e| rational_cmp(*e, *r).is_eq()));

    if roots.is_empty() {
        steps.push("\\varnothing".to_string());
        return Some(EquationSolution::TextualCertified {
            summary: format!("no solution ({})", domain_plain),
            steps,
            certificate: SolveCertificate {
                family: "abs-rational-equation".to_string(),
                domain: domain_interval_set_from_exclusions(&excluded),
                result_points: Vec::new(),
                result_intervals: IntervalSet::empty(),
                replay: SolveCertificateReplay::AbsRationalEquation {
                    expr,
                    rhs,
                    excluded,
                },
            },
        });
    }

    steps.push(format!(
        "x \\in \\{{{}\\}}",
        roots
            .iter()
            .map(|r| rational_to_latex(*r))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Some(EquationSolution::TextualCertified {
        summary: format!(
            "x in {{{}}} ({})",
            roots
                .iter()
                .map(|r| rational_to_plain(*r))
                .collect::<Vec<_>>()
                .join(", "),
            domain_plain
        ),
        steps,
        certificate: SolveCertificate {
            family: "abs-rational-equation".to_string(),
            domain: domain_interval_set_from_exclusions(&excluded),
            result_points: roots.clone(),
            result_intervals: singleton_interval_set(&roots),
            replay: SolveCertificateReplay::AbsRationalEquation {
                expr,
                rhs,
                excluded,
            },
        },
    })
}

fn solve_absolute_value_rational_inequality(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    if !contains_inequality_symbol(&compact) {
        return None;
    }

    let (mut left, mut op, mut right) = split_inequality_once(&compact)?;
    let inner = if let Some(inner) = extract_abs_inner(left) {
        inner
    } else if let Some(inner) = extract_abs_inner(right) {
        std::mem::swap(&mut left, &mut right);
        op = op.flip();
        inner
    } else {
        return None;
    };

    if !inner.contains('/') {
        return None;
    }

    let rhs = parse_number(right)?;
    let expr = parse_rational_expr_1d(inner)?;
    let excluded = denominator_exclusions(expr.den)?;
    let domain_set = domain_interval_set_from_exclusions(&excluded);
    let domain_plain = format_domain_plain(&excluded);
    let domain_latex = format_domain_latex(&excluded);
    let mut steps = vec![equation_to_latex(equation), domain_latex];

    if rhs.num < 0 {
        let summary = match op {
            InequalityOp::Lt | InequalityOp::Le => format!("no solution ({})", domain_plain),
            InequalityOp::Gt | InequalityOp::Ge => {
                format!("x in {} ({})", domain_set.to_plain_union(), domain_plain)
            }
        };
        if matches!(op, InequalityOp::Gt | InequalityOp::Ge) {
            steps.push(format!("x \\in {}", domain_set.to_latex_union()));
        } else {
            steps.push("\\varnothing".to_string());
        }
        return Some(EquationSolution::TextualCertified {
            summary,
            steps,
            certificate: SolveCertificate {
                family: "abs-rational-inequality".to_string(),
                domain: domain_set,
                result_points: Vec::new(),
                result_intervals: if matches!(op, InequalityOp::Gt | InequalityOp::Ge) {
                    domain_interval_set_from_exclusions(&excluded)
                } else {
                    IntervalSet::empty()
                },
                replay: SolveCertificateReplay::AbsRationalInequality {
                    expr,
                    op,
                    rhs,
                    excluded,
                },
            },
        });
    }

    let solved = match op {
        InequalityOp::Le | InequalityOp::Lt => {
            let low = solve_rational_inequality_core(expr, if matches!(op, InequalityOp::Le) { InequalityOp::Ge } else { InequalityOp::Gt }, rhs.negate())?;
            let high = solve_rational_inequality_core(expr, op, rhs)?;
            DomainIneqSolution {
                intervals: low.intervals.intersect(&high.intervals),
                excluded: merge_exclusions(low.excluded, high.excluded),
            }
        }
        InequalityOp::Ge | InequalityOp::Gt => {
            let low = solve_rational_inequality_core(expr, if matches!(op, InequalityOp::Ge) { InequalityOp::Le } else { InequalityOp::Lt }, rhs.negate())?;
            let high = solve_rational_inequality_core(expr, op, rhs)?;
            DomainIneqSolution {
                intervals: low.intervals.union(&high.intervals),
                excluded: merge_exclusions(low.excluded, high.excluded),
            }
        }
    };

    if solved.intervals.is_empty() {
        steps.push("\\varnothing".to_string());
        return Some(EquationSolution::TextualCertified {
            summary: format!("no solution ({})", domain_plain),
            steps,
            certificate: SolveCertificate {
                family: "abs-rational-inequality".to_string(),
                domain: domain_set,
                result_points: Vec::new(),
                result_intervals: IntervalSet::empty(),
                replay: SolveCertificateReplay::AbsRationalInequality {
                    expr,
                    op,
                    rhs,
                    excluded,
                },
            },
        });
    }

    steps.push(format!("x \\in {}", solved.intervals.to_latex_union()));
    Some(EquationSolution::TextualCertified {
        summary: format!("x in {} ({})", solved.intervals.to_plain_union(), domain_plain),
        steps,
        certificate: SolveCertificate {
            family: "abs-rational-inequality".to_string(),
            domain: domain_set,
            result_points: Vec::new(),
            result_intervals: solved.intervals,
            replay: SolveCertificateReplay::AbsRationalInequality {
                expr,
                op,
                rhs,
                excluded,
            },
        },
    })
}

fn solve_rational_equation_roots(expr: RationalExpr1D, rhs: Rational) -> Option<Vec<Rational>> {
    let reduced = shift_constant_from_ratio(expr, rhs)?;
    let mut roots = polynomial_real_roots(reduced.num)?;
    let excluded = denominator_exclusions(reduced.den)?;
    roots.retain(|r| !excluded.iter().any(|e| rational_cmp(*e, *r).is_eq()));
    Some(roots)
}

fn solve_rational_inequality_core(expr: RationalExpr1D, op: InequalityOp, rhs: Rational) -> Option<DomainIneqSolution> {
    let reduced = shift_constant_from_ratio(expr, rhs)?;
    rational_poly2_over_linear_intervals(reduced, op)
}

fn merge_exclusions(mut left: Vec<Rational>, right: Vec<Rational>) -> Vec<Rational> {
    left.extend(right);
    sorted_unique_points(left)
}

fn domain_interval_set_from_exclusions(excluded: &[Rational]) -> IntervalSet {
    if excluded.is_empty() {
        return IntervalSet::all_real();
    }
    let points = excluded
        .iter()
        .map(|v| Interval {
            lower: Some((*v, true)),
            upper: Some((*v, true)),
            is_empty: false,
        })
        .collect::<Vec<_>>();
    IntervalSet::all_real().difference(&IntervalSet::from_intervals(points))
}

// ─── Radical (square root) solver ─────────────────────────────────────────────

fn extract_sqrt_inner(s: &str) -> Option<&str> {
    if let Some(rest) = s.strip_prefix("sqrt(") {
        if let Some(inner) = rest.strip_suffix(')') {
            if !inner.is_empty() {
                return Some(inner);
            }
        }
    }
    None
}

fn eval_quadratic_sign(poly: QuadraticPoly, x: Rational) -> Option<i8> {
    let x2 = x.mul(x)?;
    let ax2 = poly.a.mul(x2)?;
    let bx = poly.b.mul(x)?;
    let val = ax2.add(bx)?.add(poly.c)?;
    if val.num > 0 {
        Some(1)
    } else if val.num < 0 {
        Some(-1)
    } else {
        Some(0)
    }
}

/// Sign-chart-based domain computation: returns the set where poly(x) >= 0.
fn poly_nonneg_domain(poly: QuadraticPoly) -> Option<IntervalSet> {
    let roots = polynomial_real_roots(poly)?;
    let critical = sorted_unique_points(roots.clone());
    let mut out = Vec::new();

    // Root points satisfy f(x) = 0 >= 0 — always included.
    for r in &roots {
        out.push(Interval { lower: Some((*r, true)), upper: Some((*r, true)), is_empty: false });
    }

    // Open intervals between consecutive critical points.
    for window in critical.windows(2) {
        let sample = sample_between(window[0], window[1]);
        if let Some(sign) = eval_quadratic_sign(poly, sample) {
            if sign > 0 {
                out.push(Interval {
                    lower: Some((window[0], false)),
                    upper: Some((window[1], false)),
                    is_empty: false,
                });
            }
        }
    }

    // Region to the left of the smallest critical point.
    if let Some(first) = critical.first().copied() {
        let sample = sample_left_of(first);
        if let Some(sign) = eval_quadratic_sign(poly, sample) {
            if sign > 0 {
                out.push(Interval { lower: None, upper: Some((first, false)), is_empty: false });
            }
        }
    }

    // Region to the right of the largest critical point.
    if let Some(last) = critical.last().copied() {
        let sample = sample_right_of(last);
        if let Some(sign) = eval_quadratic_sign(poly, sample) {
            if sign > 0 {
                out.push(Interval { lower: Some((last, false)), upper: None, is_empty: false });
            }
        }
    }

    // No critical points: constant sign everywhere.
    if critical.is_empty() {
        let sample = Rational::zero();
        return match eval_quadratic_sign(poly, sample) {
            Some(sign) if sign >= 0 => Some(IntervalSet::all_real()),
            _ => Some(IntervalSet::empty()),
        };
    }

    Some(IntervalSet::from_intervals(out))
}

fn format_radical_domain_plain(domain: &IntervalSet) -> String {
    if domain.is_empty() {
        return "domain: no real values".to_string();
    }
    let intervals = &domain.intervals;
    if intervals.len() == 1 {
        let i = intervals[0];
        if i.lower.is_none() && i.upper.is_none() {
            return "domain: all real numbers".to_string();
        }
        // [N, +inf) → "x >= N"
        if i.upper.is_none() {
            if let Some((v, true)) = i.lower {
                return format!("domain: x >= {}", rational_to_plain(v));
            }
        }
        // (-inf, N] → "x <= N"
        if i.lower.is_none() {
            if let Some((v, true)) = i.upper {
                return format!("domain: x <= {}", rational_to_plain(v));
            }
        }
    }
    format!("domain: x in {}", domain.to_plain_union())
}

fn format_radical_domain_latex(domain: &IntervalSet) -> String {
    if domain.is_empty() {
        return "\\text{domain}: \\varnothing".to_string();
    }
    let intervals = &domain.intervals;
    if intervals.len() == 1 {
        let i = intervals[0];
        if i.lower.is_none() && i.upper.is_none() {
            return "\\text{domain}: x \\in (-\\infty, +\\infty)".to_string();
        }
        if i.upper.is_none() {
            if let Some((v, true)) = i.lower {
                return format!("\\text{{domain}}: x \\ge {}", rational_to_latex(v));
            }
        }
        if i.lower.is_none() {
            if let Some((v, true)) = i.upper {
                return format!("\\text{{domain}}: x \\le {}", rational_to_latex(v));
            }
        }
    }
    format!("\\text{{domain}}: x \\in {}", domain.to_latex_union())
}

/// Sign chart for the squaring step: returns the interval set where shifted(x) OP 0.
fn poly_sign_chart_intervals(shifted: QuadraticPoly, op: InequalityOp) -> Option<IntervalSet> {
    let roots = polynomial_real_roots(shifted)?;
    let critical = sorted_unique_points(roots.clone());
    let mut out = Vec::new();

    if matches!(op, InequalityOp::Le | InequalityOp::Ge) {
        for r in &roots {
            out.push(Interval { lower: Some((*r, true)), upper: Some((*r, true)), is_empty: false });
        }
    }

    for window in critical.windows(2) {
        let sample = sample_between(window[0], window[1]);
        if let Some(sign) = eval_quadratic_sign(shifted, sample) {
            if sign_matches(sign, op) {
                out.push(Interval {
                    lower: Some((window[0], false)),
                    upper: Some((window[1], false)),
                    is_empty: false,
                });
            }
        }
    }

    if let Some(first) = critical.first().copied() {
        let sample = sample_left_of(first);
        if let Some(sign) = eval_quadratic_sign(shifted, sample) {
            if sign_matches(sign, op) {
                out.push(Interval { lower: None, upper: Some((first, false)), is_empty: false });
            }
        }
    }

    if let Some(last) = critical.last().copied() {
        let sample = sample_right_of(last);
        if let Some(sign) = eval_quadratic_sign(shifted, sample) {
            if sign_matches(sign, op) {
                out.push(Interval { lower: Some((last, false)), upper: None, is_empty: false });
            }
        }
    }

    if critical.is_empty() {
        let sample = Rational::zero();
        if let Some(sign) = eval_quadratic_sign(shifted, sample) {
            if sign_matches(sign, op) {
                return Some(IntervalSet::all_real());
            }
        }
        return Some(IntervalSet::empty());
    }

    Some(IntervalSet::from_intervals(out))
}

fn solve_radical_equation(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    if !compact.contains("sqrt(") {
        return None;
    }
    // Must be an equation, not an inequality.
    if contains_inequality_symbol(&compact) {
        return None;
    }

    let mut parts = compact.split('=');
    let left = parts.next()?;
    let right = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let (inner, rhs) = if let Some(inner) = extract_sqrt_inner(left) {
        (inner, parse_number(right)?)
    } else if let Some(inner) = extract_sqrt_inner(right) {
        (inner, parse_number(left)?)
    } else {
        return None;
    };

    let (a, b, c) = parse_polynomial_up_to_quadratic(inner)?;
    let radicand = QuadraticPoly { a, b, c };

    // sqrt(...) >= 0, so rhs < 0 means no solution.
    if rhs.num < 0 {
        return Some(EquationSolution::Textual {
            summary: "no solution".to_string(),
            steps: vec![equation_to_latex(equation), "\\sqrt{\\cdot} \\ge 0".to_string()],
        });
    }

    // Square both sides: f(x) = rhs^2 → f(x) - rhs^2 = 0.
    let rhs_sq = rhs.mul(rhs)?;
    let shifted = QuadraticPoly { a: radicand.a, b: radicand.b, c: radicand.c.sub(rhs_sq)? };
    let candidates = polynomial_real_roots(shifted)?;

    // Domain: where radicand(x) >= 0.
    let domain = poly_nonneg_domain(radicand)?;
    let domain_plain = format_radical_domain_plain(&domain);
    let domain_latex = format_radical_domain_latex(&domain);

    // Filter: keep only candidates in the domain (extraneous roots from squaring are reflections).
    let mut valid: Vec<Rational> = candidates
        .into_iter()
        .filter(|r| eval_quadratic_sign(radicand, *r).map(|s| s >= 0).unwrap_or(false))
        .collect();
    valid.sort_by(|l, r| rational_cmp(*l, *r));
    valid.dedup_by(|l, r| rational_cmp(*l, *r).is_eq());

    let mut steps = vec![equation_to_latex(equation), domain_latex];
    if valid.is_empty() {
        steps.push("\\varnothing".to_string());
        return Some(EquationSolution::TextualCertified {
            summary: format!("no solution ({})", domain_plain),
            steps,
            certificate: SolveCertificate {
                family: "radical-equation".to_string(),
                domain: domain.clone(),
                result_points: Vec::new(),
                result_intervals: IntervalSet::empty(),
                replay: SolveCertificateReplay::RadicalEquation { radicand, rhs },
            },
        });
    }

    let summary = if valid.len() == 1 {
        format!("x = {} ({})", rational_to_plain(valid[0]), domain_plain)
    } else {
        format!(
            "x in {{{}}} ({})",
            valid.iter().map(|r| rational_to_plain(*r)).collect::<Vec<_>>().join(", "),
            domain_plain
        )
    };
    steps.push(format!(
        "x \\in \\{{{}\\}}",
        valid.iter().map(|r| rational_to_latex(*r)).collect::<Vec<_>>().join(", ")
    ));
    Some(EquationSolution::TextualCertified {
        summary,
        steps,
        certificate: SolveCertificate {
            family: "radical-equation".to_string(),
            domain: domain.clone(),
            result_points: valid.clone(),
            result_intervals: singleton_interval_set(&valid),
            replay: SolveCertificateReplay::RadicalEquation { radicand, rhs },
        },
    })
}

fn solve_radical_inequality(equation: &str) -> Option<EquationSolution> {
    let compact = equation.replace(' ', "");
    if !compact.contains("sqrt(") {
        return None;
    }
    if !contains_inequality_symbol(&compact) {
        return None;
    }

    let (left_part, split_op, right_part) = split_inequality_once(&compact)?;

    // Normalise: sqrt(...) on the left side.
    let (inner, op, rhs_str) = if extract_sqrt_inner(left_part).is_some() {
        (extract_sqrt_inner(left_part)?, split_op, right_part)
    } else if extract_sqrt_inner(right_part).is_some() {
        (extract_sqrt_inner(right_part)?, split_op.flip(), left_part)
    } else {
        return None;
    };

    let rhs = parse_number(rhs_str)?;
    let (a, b, c) = parse_polynomial_up_to_quadratic(inner)?;
    let radicand = QuadraticPoly { a, b, c };

    let domain = poly_nonneg_domain(radicand)?;
    let domain_plain = format_radical_domain_plain(&domain);
    let domain_latex = format_radical_domain_latex(&domain);

    // sqrt(f(x)) is always >= 0. Handle negative RHS shortcuts.
    let result_intervals = if rhs.num < 0 {
        match op {
            // sqrt >= negative is always satisfied within the domain.
            InequalityOp::Gt | InequalityOp::Ge => domain.clone(),
            // sqrt < negative is never satisfied.
            InequalityOp::Lt | InequalityOp::Le => IntervalSet::empty(),
        }
    } else {
        // rhs >= 0: square both sides.
        // sqrt(f(x)) OP c  →  f(x) - c^2 OP 0  (valid since sqrt is monotone and non-negative)
        let rhs_sq = rhs.mul(rhs)?;
        let shifted = QuadraticPoly { a: radicand.a, b: radicand.b, c: radicand.c.sub(rhs_sq)? };
        let squared_set = poly_sign_chart_intervals(shifted, op)?;
        domain.intersect(&squared_set)
    };

    let mut steps = vec![equation_to_latex(equation), domain_latex];
    if result_intervals.is_empty() {
        steps.push("\\varnothing".to_string());
        return Some(EquationSolution::TextualCertified {
            summary: format!("no solution ({})", domain_plain),
            steps,
            certificate: SolveCertificate {
                family: "radical-inequality".to_string(),
                domain: domain.clone(),
                result_points: Vec::new(),
                result_intervals: IntervalSet::empty(),
                replay: SolveCertificateReplay::RadicalInequality { radicand, op, rhs },
            },
        });
    }

    steps.push(format!("x \\in {}", result_intervals.to_latex_union()));
    Some(EquationSolution::TextualCertified {
        summary: format!("x in {} ({})", result_intervals.to_plain_union(), domain_plain),
        steps,
        certificate: SolveCertificate {
            family: "radical-inequality".to_string(),
            domain: domain.clone(),
            result_points: Vec::new(),
            result_intervals,
            replay: SolveCertificateReplay::RadicalInequality { radicand, op, rhs },
        },
    })
}

fn extract_abs_inner(side: &str) -> Option<&str> {
    if side.starts_with('|') && side.ends_with('|') && side.len() > 2 {
        return Some(&side[1..side.len() - 1]);
    }
    if side.starts_with("abs(") && side.ends_with(')') && side.len() > 5 {
        return Some(&side[4..side.len() - 1]);
    }
    None
}

fn parse_linear_xy_equation(equation: &str) -> Option<(Rational, Rational, Rational)> {
    let compact = equation.replace(' ', "");
    let mut parts = compact.split('=');
    let left = parts.next()?;
    let right = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let (a1, b1, c1) = parse_linear_xy_expr(left)?;
    let (a2, b2, c2) = parse_linear_xy_expr(right)?;
    Some((a1.sub(a2)?, b1.sub(b2)?, c2.sub(c1)?))
}

fn parse_linear_xy_expr(expr: &str) -> Option<(Rational, Rational, Rational)> {
    if expr.is_empty() {
        return None;
    }
    let normalized = expr.replace('-', "+-");
    let normalized = if let Some(rest) = normalized.strip_prefix("+-") {
        format!("-{}", rest)
    } else {
        normalized
    };

    let mut ax = Rational::zero();
    let mut by = Rational::zero();
    let mut c = Rational::zero();

    for term in normalized.split('+').filter(|t| !t.trim().is_empty()) {
        if let Some(coeff_raw) = term.strip_suffix('x') {
            ax = ax.add(parse_symbolic_coeff(coeff_raw)?)?;
        } else if let Some(coeff_raw) = term.strip_suffix('y') {
            by = by.add(parse_symbolic_coeff(coeff_raw)?)?;
        } else {
            c = c.add(parse_number(term)?)?;
        }
    }

    Some((ax, by, c))
}

fn parse_factorized_zero_form(
    left: &str,
    right: &str,
) -> Option<((Rational, Rational), (Rational, Rational))> {
    if right != "0" {
        return None;
    }
    let lhs = left.replace('*', "");
    if !lhs.starts_with('(') {
        return None;
    }
    let close1 = lhs.find(')')?;
    let first = &lhs[1..close1];
    let rest = &lhs[close1 + 1..];
    if !rest.starts_with('(') || !rest.ends_with(')') {
        return None;
    }
    let second = &rest[1..rest.len() - 1];
    let f1 = parse_linear_x_expr(first)?;
    let f2 = parse_linear_x_expr(second)?;
    Some((f1, f2))
}

fn parse_linear_x_expr(expr: &str) -> Option<(Rational, Rational)> {
    if expr.is_empty() {
        return None;
    }
    let normalized = expr.replace('-', "+-");
    let normalized = if let Some(rest) = normalized.strip_prefix("+-") {
        format!("-{}", rest)
    } else {
        normalized
    };

    let mut a = Rational::zero();
    let mut b = Rational::zero();
    for term in normalized.split('+').filter(|t| !t.trim().is_empty()) {
        if let Some(coeff_raw) = term.strip_suffix('x') {
            a = a.add(parse_symbolic_coeff(coeff_raw)?)?;
        } else {
            b = b.add(parse_number(term)?)?;
        }
    }
    Some((a, b))
}

fn linear_factor_to_latex(a: Rational, b: Rational) -> String {
    let a_s = rational_to_latex(a);
    if b.is_zero() {
        return format!("{}x", a_s);
    }
    if b.num > 0 {
        let b_s = rational_to_latex(b);
        format!("{}x + {}", a_s, b_s)
    } else {
        format!("{}x - {}", a_s, rational_to_latex(b.abs()))
    }
}

fn parse_polynomial_up_to_quadratic(expr: &str) -> Option<(Rational, Rational, Rational)> {
    if expr.is_empty() {
        return None;
    }

    let normalized = expr.replace('-', "+-");
    let normalized = if let Some(rest) = normalized.strip_prefix("+-") {
        format!("-{}", rest)
    } else {
        normalized
    };

    let mut a = Rational::zero();
    let mut b = Rational::zero();
    let mut c = Rational::zero();

    for raw_term in normalized.split('+') {
        let term = raw_term.trim();
        if term.is_empty() {
            continue;
        }

        if let Some(coeff_raw) = term.strip_suffix("x^2") {
            let coeff = parse_symbolic_coeff(coeff_raw)?;
            a = a.add(coeff)?;
            continue;
        }

        if let Some(coeff_raw) = term.strip_suffix('x') {
            if term.contains('^') {
                return None;
            }
            let coeff = parse_symbolic_coeff(coeff_raw)?;
            b = b.add(coeff)?;
            continue;
        }

        c = c.add(parse_number(term)?)?;
    }

    Some((a, b, c))
}

fn parse_symbolic_coeff(text: &str) -> Option<Rational> {
    match text {
        "" | "+" => Rational::new(1, 1),
        "-" => Rational::new(-1, 1),
        _ => parse_number(text),
    }
}

fn parse_number(text: &str) -> Option<Rational> {
    Rational::from_decimal_str(text)
}

fn equation_to_latex(equation: &str) -> String {
    equation
        .replace("x^2", "x^{2}")
        .replace('*', " \\cdot ")
}

fn compute_latex_enabled() -> bool {
    if let Some(value) = COMPUTE_LATEX_OVERRIDE.with(|slot| slot.get()) {
        return value;
    }

    std::env::var("CSIF_COMPUTE_LATEX")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

#[cfg(test)]
fn set_compute_latex_override_for_current_thread(value: Option<bool>) {
    COMPUTE_LATEX_OVERRIDE.with(|slot| slot.set(value));
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
    use std::sync::{Mutex, OnceLock};

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

    fn with_latex_enabled<T>(f: impl FnOnce() -> T) -> T {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        set_compute_latex_override_for_current_thread(Some(true));
        let out = f();
        set_compute_latex_override_for_current_thread(None);
        drop(guard);
        out
    }

    fn solution_certificate(solution: &EquationSolution) -> &SolveCertificate {
        match solution {
            EquationSolution::TextualCertified { certificate, .. } => certificate,
            _ => panic!("expected certified textual solution"),
        }
    }

    fn math_certificate(proof: &ProofCertificate) -> &SolveCertificate {
        match proof {
            ProofCertificate::Math(certificate) => certificate,
            _ => panic!("expected math proof certificate"),
        }
    }

    fn language_certificate(proof: &ProofCertificate) -> &LanguageCertificate {
        match proof {
            ProofCertificate::Language(certificate) => certificate,
            _ => panic!("expected language proof certificate"),
        }
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
    fn causes_relation_respects_depth_limit_policy() {
        let bank_path = temp_bank_path("causes_depth_limit");
        let grammar_path = temp_grammar_path("causes_depth_limit");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        assert_eq!(
            agent.teach("rain causes wet ground"),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("wet ground causes slippery"),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("slippery causes accident"),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("accident causes traffic jam"),
            "[TEACHING] Knowledge crystallized."
        );

        assert_eq!(
            agent.query("Does rain cause accident?"),
            "[CRYSTAL] YES: rain causes accident."
        );
        assert_eq!(
            agent.query("Does rain cause traffic jam?"),
            "[CRYSTAL] NO: I cannot establish that rain causes traffic jam."
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn explain_query_returns_path_depth_and_confidence_for_relation_confirmation() {
        let bank_path = temp_bank_path("explain_query");
        let grammar_path = temp_grammar_path("explain_query");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        assert_eq!(
            agent.teach("a whale is a mammal"),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("a mammal is an animal"),
            "[TEACHING] Knowledge crystallized."
        );

        let explain = agent.explain_query("Is a whale an animal?");
        assert_eq!(explain.intent, "confirm_relation");
        assert_eq!(explain.relation.as_deref(), Some("is_a"));
        assert!(explain.path.len() >= 3);
        assert_eq!(explain.path.first().map(String::as_str), Some("whale"));
        assert_eq!(explain.path.last().map(String::as_str), Some("animal"));
        assert_eq!(explain.depth_limit, None);
        assert!(explain.confidence.unwrap_or_default() > 0.9);
        let request_time = explain
            .request_time_context
            .as_ref()
            .expect("expected explain request_time_context");
        assert_eq!(request_time.timezone, "UTC");
        assert!(request_time.unix_ms > 0);
        assert!(
            request_time.request_received_at.ends_with('Z')
                || request_time.request_received_at.ends_with("+00:00")
        );
        let route_audit = explain
            .route_audit
            .as_ref()
            .expect("expected explain route_audit");
        assert_eq!(route_audit.relation.as_deref(), Some("is_a"));
        assert_eq!(route_audit.subject.as_deref(), Some("whale"));
        assert_eq!(route_audit.object.as_deref(), Some("animal"));
        assert!(!route_audit.tried.is_empty());
        assert!(route_audit.stop_reason.contains("path_found"));
        assert!(
            explain
                .considered_contradictions
                .iter()
                .any(|line| line.contains("No high-phase contradiction"))
        );

        let query_payload = agent.query_with_certificate("Is a whale an animal?");
        assert!(query_payload.request_time_context.is_some());
        assert!(query_payload.route_audit.is_some());

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn negative_relation_query_persists_explicit_anti_lobe_edge() {
        let bank_path = temp_bank_path("anti_lobe_persist");
        let grammar_path = temp_grammar_path("anti_lobe_persist");
        let anti_bank_path = anti_lobe_bank_path(&bank_path);
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        assert_eq!(
            agent.teach("a whale is a mammal"),
            "[TEACHING] Knowledge crystallized."
        );

        let first = agent.query_with_certificate("Is a whale a reptile?");
        assert!(first.answer.contains("NO:"));
        let first_audit = first
            .route_audit
            .as_ref()
            .expect("expected route audit on first negative query");
        assert_eq!(first_audit.stop_reason, "no_supporting_path");

        let anti_edge = agent
            .anti_lobe
            .edges
            .values()
            .find(|edge| edge.relation == "not_is_a")
            .expect("expected persisted AntiLobe edge");
        assert_eq!(anti_edge.lobe, "AntiLobe");
        assert!(
            anti_edge
                .trajectory
                .last()
                .map(|event| (event.phase - PI).abs() < 0.0001)
                .unwrap_or(false)
        );

        agent.save().unwrap();
        let reloaded = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();
        assert!(reloaded.has_explicit_negative_relation("whale", "reptile", RelationType::IsA));
        assert!(anti_bank_path.exists());
        assert!(
            reloaded
                .crystal
                .edges
                .values()
                .all(|edge| edge.relation != "not_is_a")
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(anti_bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn repeated_negative_relation_query_uses_anti_lobe_short_circuit() {
        let bank_path = temp_bank_path("anti_lobe_repeat");
        let grammar_path = temp_grammar_path("anti_lobe_repeat");
        let anti_bank_path = anti_lobe_bank_path(&bank_path);
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        assert_eq!(
            agent.teach("a whale is a mammal"),
            "[TEACHING] Knowledge crystallized."
        );

        let _ = agent.query_with_certificate("Is a whale a reptile?");
        let second = agent.query_with_certificate("Is a whale a reptile?");
        let second_audit = second
            .route_audit
            .as_ref()
            .expect("expected route audit on repeated negative query");
        assert_eq!(second_audit.stop_reason, "anti_lobe_negative_match");
        assert_eq!(
            second_audit.anti_lobe_bank_path.as_deref(),
            Some(anti_bank_path.to_string_lossy().as_ref())
        );
        assert!(
            second_audit
                .negative_evidence
                .iter()
                .any(|line| line.contains("Explicit AntiLobe edge observed"))
        );

        let explain = agent.explain_query("Is a whale a reptile?");
        let explain_audit = explain
            .route_audit
            .as_ref()
            .expect("expected explain route audit on repeated negative query");
        assert_eq!(explain_audit.stop_reason, "anti_lobe_negative_match");
        assert_eq!(
            explain_audit.anti_lobe_bank_path.as_deref(),
            Some(anti_bank_path.to_string_lossy().as_ref())
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(anti_bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn play_cycle_crystallizes_success_and_suppresses_repeated_failed_hypotheses() {
        let bank_path = temp_bank_path("play_cycle");
        let grammar_path = temp_grammar_path("play_cycle");
        let anti_bank_path = anti_lobe_bank_path(&bank_path);
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        assert_eq!(
            agent.teach("a whale is a mammal"),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("a mammal is an animal"),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("a mammal has vertebrate"),
            "[TEACHING] Knowledge crystallized."
        );

        let first = agent.run_play_cycle();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].relation, "is_a");
        assert_eq!(first[0].subject, "whale");
        assert_eq!(first[0].object, "animal");
        assert_eq!(first[0].outcome, PlayAttemptOutcome::SuccessCrystallized);
        assert!(
            agent
                .direct_targets_for_relation("whale", RelationType::IsA)
                .iter()
                .any(|target| target == "animal")
        );

        assert_eq!(first[1].relation, "has_property");
        assert_eq!(first[1].subject, "whale");
        assert_eq!(first[1].object, "vertebrate");
        assert_eq!(first[1].outcome, PlayAttemptOutcome::FailurePersisted);
        assert!(agent.has_explicit_negative_relation("whale", "vertebrate", RelationType::HasProperty));

        let second = agent.run_play_cycle();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].relation, "has_property");
        assert_eq!(second[0].outcome, PlayAttemptOutcome::SkippedKnownFailure);
        assert!(second[0].detail.contains(anti_bank_path.to_string_lossy().as_ref()));

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(anti_bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn teaching_true_relation_removes_obsolete_anti_lobe_edge() {
        let bank_path = temp_bank_path("anti_lobe_obsolete_cleanup");
        let grammar_path = temp_grammar_path("anti_lobe_obsolete_cleanup");
        let anti_bank_path = anti_lobe_bank_path(&bank_path);
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let _ = agent.query_with_certificate("Is a whale a reptile?");
        assert!(agent.has_explicit_negative_relation("whale", "reptile", RelationType::IsA));

        assert_eq!(
            agent.teach("a whale is a reptile"),
            "[TEACHING] Knowledge crystallized."
        );
        assert!(!agent.has_explicit_negative_relation("whale", "reptile", RelationType::IsA));

        let explain = agent.explain_query("Is a whale a reptile?");
        assert_eq!(explain.answer, "[CRYSTAL] YES: a whale is a reptile.");
        let route_audit = explain.route_audit.as_ref().expect("expected route audit");
        assert_eq!(route_audit.stop_reason, "path_found");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(anti_bank_path);
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
        let rendered = with_latex_enabled(|| render_compute_expression("(9 + 4) * 2"));
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
    fn solve_quadratic_inequality_returns_union_of_intervals() {
        let bank_path = temp_bank_path("solve_quadratic_ineq");
        let grammar_path = temp_grammar_path("solve_quadratic_ineq");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve x^2 - 5x + 6 >= 0");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in (-inf, 2] U [3, +inf)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_rational_inequality_returns_exclusion_interval_union() {
        let bank_path = temp_bank_path("solve_rational_ineq");
        let grammar_path = temp_grammar_path("solve_rational_ineq");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve (x-1)/(x+1) > 0");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in (-inf, -1) U (1, +inf) (domain: x != -1)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_rational_equation_reports_excluded_points() {
        let bank_path = temp_bank_path("solve_rational_equation_domain");
        let grammar_path = temp_grammar_path("solve_rational_equation_domain");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve (x+1)/(x-1) = 0");
        assert_eq!(
            answer.lines().next().unwrap_or(""),
            "[CRYSTAL] [SOLVE] x = -1 (domain: x != 1)"
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_mixed_polynomial_rational_inequality_reports_domain_certificate() {
        let bank_path = temp_bank_path("solve_mixed_poly_rational_ineq_domain");
        let grammar_path = temp_grammar_path("solve_mixed_poly_rational_ineq_domain");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve x^2/(x-1) >= 0");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in {0} U (1, +inf) (domain: x != 1)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_mixed_polynomial_rational_inequality_with_two_poly_roots() {
        let bank_path = temp_bank_path("solve_mixed_poly_roots_ineq_domain");
        let grammar_path = temp_grammar_path("solve_mixed_poly_roots_ineq_domain");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve (x^2-1)/(x-2) > 0");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in (-1, 1) U (2, +inf) (domain: x != 2)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_absolute_value_rational_equation_reports_domain_certificate() {
        let bank_path = temp_bank_path("solve_abs_rational_equation_domain");
        let grammar_path = temp_grammar_path("solve_abs_rational_equation_domain");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve |(x+1)/(x-1)| = 2");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in {1/3, 3} (domain: x != 1)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_absolute_value_rational_inequality_reports_domain_certificate() {
        let bank_path = temp_bank_path("solve_abs_rational_ineq_domain");
        let grammar_path = temp_grammar_path("solve_abs_rational_ineq_domain");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve |(x+1)/(x-1)| <= 1");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in (-inf, 0] (domain: x != 1)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_chained_mixed_inequality_reports_domain_certificate() {
        let bank_path = temp_bank_path("solve_chained_mixed_ineq_domain");
        let grammar_path = temp_grammar_path("solve_chained_mixed_ineq_domain");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve 0 <= x^2/(x-1) < 3");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in {0} (domain: x != 1)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_sqrt_linear_equation_reports_domain_certificate() {
        let bank_path = temp_bank_path("solve_sqrt_linear_eq");
        let grammar_path = temp_grammar_path("solve_sqrt_linear_eq");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve sqrt(x+3) = 2");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x = 1 (domain: x >= -3)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_sqrt_linear_inequality_reports_domain_certificate() {
        let bank_path = temp_bank_path("solve_sqrt_linear_ineq");
        let grammar_path = temp_grammar_path("solve_sqrt_linear_ineq");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve sqrt(x-1) <= 2");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in [1, 5] (domain: x >= 1)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_sqrt_quadratic_radicand_equation_reports_domain_certificate() {
        let bank_path = temp_bank_path("solve_sqrt_quad_eq");
        let grammar_path = temp_grammar_path("solve_sqrt_quad_eq");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve sqrt(x^2-4) = 0");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in {-2, 2} (domain: x in (-inf, -2] U [2, +inf))");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn replay_certificate_accepts_rational_radical_abs_paths() {
        let rational = solve_equation("(x-1)/(x+1)=0");
        assert!(replay_solve_certificate(solution_certificate(&rational)));

        let abs_rational = solve_equation("|(x+1)/(x-1)| <= 1");
        assert!(replay_solve_certificate(solution_certificate(&abs_rational)));

        let radical = solve_equation("sqrt(x+3)=2");
        assert!(replay_solve_certificate(solution_certificate(&radical)));
    }

    #[test]
    fn replay_certificate_rejects_tampered_solution_points() {
        let solution = solve_equation("(x-1)/(x+1)=0");
        let mut cert = solution_certificate(&solution).clone();
        cert.result_points.push(Rational::new(-1, 1).unwrap());
        cert.result_intervals = singleton_interval_set(&cert.result_points);
        assert!(!replay_solve_certificate(&cert));
    }

    #[test]
    fn replay_certificate_rejects_tampered_intervals() {
        let solution = solve_equation("|(x+1)/(x-1)| <= 1");
        let mut cert = solution_certificate(&solution).clone();
        cert.result_intervals = IntervalSet::all_real();
        assert!(!replay_solve_certificate(&cert));
    }

    #[test]
    fn query_with_certificate_exposes_solve_proof_payload() {
        let bank_path = temp_bank_path("query_with_certificate_solve");
        let grammar_path = temp_grammar_path("query_with_certificate_solve");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("Solve (x+1)/(x-1) = 0");
        assert_eq!(
            result.answer.lines().next().unwrap_or(""),
            "[CRYSTAL] [SOLVE] x = -1 (domain: x != 1)"
        );

        let cert = result
            .certificate
            .as_ref()
            .expect("solve queries should include a certificate");
        assert!(replay_solve_certificate(math_certificate(cert)));

        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("certificate").is_some());
        assert_eq!(
            json.get("certificate")
                .and_then(|c| c.get("domain"))
                .and_then(|v| v.as_str()),
            Some("math")
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_omits_proof_for_non_solve() {
        let bank_path = temp_bank_path("query_with_certificate_non_solve");
        let grammar_path = temp_grammar_path("query_with_certificate_non_solve");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        agent.teach("A whale is a mammal.");
        let result = agent.query_with_certificate("Is a whale a mammal?");
        assert_eq!(result.answer, "[CRYSTAL] YES: a whale is a mammal.");
        let cert = result
            .certificate
            .as_ref()
            .expect("language relation answers should include a certificate");
        assert!(replay_language_certificate(language_certificate(cert)));

        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("certificate").is_some());
        assert_eq!(
            json.get("certificate")
                .and_then(|c| c.get("payload"))
                .and_then(|p| p.get("parse"))
                .and_then(|p| p.get("best"))
                .and_then(|b| b.get("intent"))
                .and_then(|v| v.as_str()),
            Some("ConfirmRelation")
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_exposes_language_proof_payload() {
        let bank_path = temp_bank_path("query_with_certificate_language");
        let grammar_path = temp_grammar_path("query_with_certificate_language");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        assert_eq!(
            agent.teach("A whale is a mammal."),
            "[TEACHING] Knowledge crystallized."
        );
        assert_eq!(
            agent.teach("A mammal is an animal."),
            "[TEACHING] Knowledge crystallized."
        );

        let result = agent.query_with_certificate("What is a whale?");
        assert!(result.answer.starts_with("[CRYSTAL] "));

        let cert = result
            .certificate
            .as_ref()
            .expect("describe queries should include a language certificate");
        assert!(replay_language_certificate(language_certificate(cert)));

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(
            json.get("certificate")
                .and_then(|c| c.get("domain"))
                .and_then(|v| v.as_str()),
            Some("language")
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_parses_instruction_requests_with_lattice() {
        let bank_path = temp_bank_path("query_with_certificate_instruction");
        let grammar_path = temp_grammar_path("query_with_certificate_instruction");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("How do I restart the server?");
        assert!(result.answer.starts_with("[CRYSTAL] [PLAN]"));
        assert!(result.answer.contains("action=restart the server"));

        let cert = result
            .certificate
            .as_ref()
            .expect("instruction requests should include a language certificate");
        assert!(replay_language_certificate(language_certificate(cert)));

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(
            json.get("certificate")
                .and_then(|c| c.get("payload"))
                .and_then(|p| p.get("parse"))
                .and_then(|p| p.get("best"))
                .and_then(|b| b.get("intent"))
                .and_then(|v| v.as_str()),
            Some("InstructionRequest")
        );
        assert_eq!(
            json.get("certificate")
                .and_then(|c| c.get("payload"))
                .and_then(|p| p.get("parse"))
                .and_then(|p| p.get("ambiguity_remaining"))
                .and_then(|v| v.as_bool()),
            Some(false)
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_cleans_instruction_modifier_from_target() {
        let bank_path = temp_bank_path("query_with_certificate_instruction_modifier");
        let grammar_path = temp_grammar_path("query_with_certificate_instruction_modifier");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("How do I restart nginx safely?");
        assert!(result.answer.starts_with("[CRYSTAL] [PLAN]"));
        assert!(result.answer.contains("action=restart nginx for nginx"));
        assert!(!result.answer.contains("nginx safely"));

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_splits_combined_instruction_and_compute_prompt() {
        let bank_path = temp_bank_path("query_with_certificate_combined_prompt");
        let grammar_path = temp_grammar_path("query_with_certificate_combined_prompt");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent
            .query_with_certificate("How do I restart nginx safely? Also, what is 3 x 9?");
        assert!(result.answer.contains("[CRYSTAL] [PLAN] action=restart nginx for nginx"));
        assert!(result.answer.contains("[CRYSTAL] [COMPUTE] 3 * 9 = 27"));
        assert!(result.certificate.is_none());

        let clauses = result
            .clauses
            .as_ref()
            .expect("combined prompts should expose clause-level output");
        assert_eq!(clauses.len(), 2);
        assert!(clauses[0].certificate.is_some());
        assert!(clauses[1].certificate.is_none());
        assert_eq!(clauses[0].semantic_projection.intent, "instruction_request");
        assert_eq!(clauses[1].semantic_projection.intent, "compute_expression");
        assert!(clauses[0]
            .semantic_projection
            .meaning_tokens
            .iter()
            .any(|token| token == "instruction"));
        assert!(clauses[1]
            .semantic_projection
            .meaning_tokens
            .iter()
            .any(|token| token == "compute"));

        let summary = result
            .composite
            .as_ref()
            .expect("combined prompts should include composite summary");
        assert_eq!(summary.clause_count, 2);
        assert_eq!(summary.clauses_with_certificates, 1);
        assert_eq!(summary.verified_certificates, 1);
        assert!(summary.all_clause_certificates_verified);
        assert!(summary.intents.iter().any(|intent| intent == "instruction_request"));
        assert!(summary.intents.iter().any(|intent| intent == "compute_expression"));
        assert!(summary
            .meaning_vocabulary
            .iter()
            .any(|token| token == "instruction"));
        assert!(summary
            .meaning_vocabulary
            .iter()
            .any(|token| token == "compute"));

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_parses_negated_narrative_state() {
        let bank_path = temp_bank_path("query_with_certificate_narrative_state");
        let grammar_path = temp_grammar_path("query_with_certificate_narrative_state");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("The server is not responding.");
        assert!(result.answer.starts_with("[CRYSTAL] [TEACHING] Narrative state persisted:"));

        let cert = result
            .certificate
            .as_ref()
            .expect("narrative state should include a language certificate");
        assert!(replay_language_certificate(language_certificate(cert)));

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(
            json.get("certificate")
                .and_then(|c| c.get("payload"))
                .and_then(|p| p.get("parse"))
                .and_then(|p| p.get("best"))
                .and_then(|b| b.get("intent"))
                .and_then(|v| v.as_str()),
            Some("NarrativeState")
        );

        let has_state_edge = agent
            .crystal
            .edges
            .values()
            .any(|edge| edge.relation == "state_not_at");
        assert!(has_state_edge);

        let has_time_anchor = agent
            .crystal
            .edges
            .values()
            .any(|edge| edge.relation == "observed_at");
        assert!(has_time_anchor);

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_triggers_clarification_for_ambiguous_bare_imperative() {
        let bank_path = temp_bank_path("query_with_certificate_ambiguous_instruction");
        let grammar_path = temp_grammar_path("query_with_certificate_ambiguous_instruction");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("Restart the server");
        assert!(result.answer.starts_with("[NEEDS_INPUT] Do you want me to treat this as an instruction"));

        let cert = result
            .certificate
            .as_ref()
            .expect("ambiguous imperative should include a language certificate");
        assert!(replay_language_certificate(language_certificate(cert)));

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(
            json.get("certificate")
                .and_then(|c| c.get("payload"))
                .and_then(|p| p.get("parse"))
                .and_then(|p| p.get("ambiguity_remaining"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(
            json.get("certificate")
                .and_then(|c| c.get("payload"))
                .and_then(|p| p.get("parse"))
                .and_then(|p| p.get("clarification_trigger"))
                .is_some()
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_persists_narrative_event_edges() {
        let bank_path = temp_bank_path("query_with_certificate_narrative_event");
        let grammar_path = temp_grammar_path("query_with_certificate_narrative_event");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("rain caused flooding");
        assert!(result.answer.starts_with("[CRYSTAL] [TEACHING] Narrative event persisted:"));

        let cert = result
            .certificate
            .as_ref()
            .expect("narrative event should include a language certificate");
        assert!(replay_language_certificate(language_certificate(cert)));

        let has_event_edge = agent
            .crystal
            .edges
            .values()
            .any(|edge| edge.relation == "event_at");
        assert!(has_event_edge);

        let has_event_object_edge = agent
            .crystal
            .edges
            .values()
            .any(|edge| edge.relation == "event_object");
        assert!(has_event_object_edge);

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_repeated_narrative_state_appends_temporal_trajectory() {
        let bank_path = temp_bank_path("query_with_certificate_repeated_narrative_state");
        let grammar_path = temp_grammar_path("query_with_certificate_repeated_narrative_state");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let first = agent.query_with_certificate("The server is not responding.");
        let second = agent.query_with_certificate("The server is not responding.");
        assert!(first.answer.starts_with("[CRYSTAL] [TEACHING] Narrative state persisted:"));
        assert!(second.answer.starts_with("[CRYSTAL] [TEACHING] Narrative state persisted:"));

        let repeated_state_edge = agent
            .crystal
            .edges
            .values()
            .find(|edge| edge.relation == "state_not_at" && edge.trajectory.len() >= 2);
        assert!(
            repeated_state_edge.is_some(),
            "expected repeated state_not_at edge trajectory growth"
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_rejects_tampered_instruction_safe_actions() {
        let bank_path = temp_bank_path("query_with_certificate_tampered_instruction");
        let grammar_path = temp_grammar_path("query_with_certificate_tampered_instruction");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("How do I restart the server?");
        let mut cert = result
            .certificate
            .clone()
            .expect("instruction query should include a language certificate");

        if let ProofCertificate::Language(language) = &mut cert {
            if let LanguageCertificateReplay::InstructionRequest { safe_actions, .. } = &mut language.replay {
                safe_actions.clear();
            } else {
                panic!("expected instruction replay payload");
            }
        } else {
            panic!("expected language proof certificate");
        }

        assert!(!verify_proof_certificate(&cert));

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn query_with_certificate_rejects_tampered_instruction_execution_policy() {
        let bank_path = temp_bank_path("query_with_certificate_tampered_instruction_policy");
        let grammar_path = temp_grammar_path("query_with_certificate_tampered_instruction_policy");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("How do I restart the server?");
        let mut cert = result
            .certificate
            .clone()
            .expect("instruction query should include a language certificate");

        if let ProofCertificate::Language(language) = &mut cert {
            if let LanguageCertificateReplay::InstructionRequest { execution_policy, .. } = &mut language.replay {
                execution_policy.allow_mutation = true;
            } else {
                panic!("expected instruction replay payload");
            }
        } else {
            panic!("expected language proof certificate");
        }

        assert!(!verify_proof_certificate(&cert));

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn evaluate_instruction_execution_allows_inspect_action() {
        let bank_path = temp_bank_path("evaluate_instruction_execution_allow");
        let grammar_path = temp_grammar_path("evaluate_instruction_execution_allow");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("How do I restart the server?");
        let cert = result
            .certificate
            .as_ref()
            .expect("instruction query should include certificate");

        let decision = evaluate_instruction_execution(cert, 0, None);
        assert!(decision.ok);
        assert!(decision.executed);
        assert!(!decision.requires_approval);
        assert_eq!(decision.action_kind.as_deref(), Some("inspect"));

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn evaluate_instruction_execution_requires_approval_for_mutation() {
        let bank_path = temp_bank_path("evaluate_instruction_execution_mutation_gate");
        let grammar_path = temp_grammar_path("evaluate_instruction_execution_mutation_gate");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("How do I restart the server?");
        let cert = result
            .certificate
            .as_ref()
            .expect("instruction query should include certificate");

        let decision = evaluate_instruction_execution(cert, 1, None);
        assert!(!decision.ok);
        assert!(!decision.executed);
        assert!(decision.requires_approval);
        assert_eq!(decision.action_kind.as_deref(), Some("mutate"));

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn evaluate_instruction_execution_allows_mutation_with_valid_approval_token() {
        let bank_path = temp_bank_path("evaluate_instruction_execution_mutation_approved");
        let grammar_path = temp_grammar_path("evaluate_instruction_execution_mutation_approved");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("How do I restart the server?");
        let cert = result
            .certificate
            .as_ref()
            .expect("instruction query should include certificate");

        let decision = evaluate_instruction_execution_with_secret(
            cert,
            1,
            Some("approve-me"),
            Some("approve-me"),
        );
        assert!(decision.ok);
        assert!(decision.executed);
        assert!(!decision.requires_approval);
        assert_eq!(decision.action_kind.as_deref(), Some("mutate"));

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn evaluate_instruction_execution_rejects_tampered_instruction_certificate() {
        let bank_path = temp_bank_path("evaluate_instruction_execution_tampered");
        let grammar_path = temp_grammar_path("evaluate_instruction_execution_tampered");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let result = agent.query_with_certificate("How do I restart the server?");
        let mut cert = result
            .certificate
            .clone()
            .expect("instruction query should include certificate");

        if let ProofCertificate::Language(language) = &mut cert {
            if let LanguageCertificateReplay::InstructionRequest { safe_actions, .. } = &mut language.replay {
                safe_actions.clear();
            } else {
                panic!("expected instruction replay payload");
            }
        } else {
            panic!("expected language proof certificate");
        }

        let decision = evaluate_instruction_execution(&cert, 0, None);
        assert!(!decision.ok);
        assert!(!decision.executed);
        assert!(decision.reason.contains("invalid or tampered"));

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn teach_chain_then_query_enforces_relation_rules_under_depth() {
        let bank_path = temp_bank_path("teach_chain_depth_rules");
        let grammar_path = temp_grammar_path("teach_chain_depth_rules");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        for fact in [
            "A peregrine is a falcon.",
            "A falcon is a raptor.",
            "A raptor is a bird.",
            "A bird is an animal.",
            "a bird has feathers",
        ] {
            assert_eq!(agent.teach(fact), "[TEACHING] Knowledge crystallized.");
        }

        let is_a_result = agent.query_with_certificate("Is a peregrine an animal?");
        assert_eq!(is_a_result.answer, "[CRYSTAL] YES: a peregrine is an animal.");
        let relation_cert = is_a_result
            .certificate
            .as_ref()
            .expect("relation confirmation should include certificate");
        assert!(replay_language_certificate(language_certificate(relation_cert)));

        let property_result = agent.query("Does a peregrine have feathers?");
        assert_eq!(
            property_result,
            "[CRYSTAL] NO: I cannot establish that a peregrine is feathers."
        );

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_nxn_linear_system_returns_exact_solution() {
        let bank_path = temp_bank_path("solve_nxn");
        let grammar_path = temp_grammar_path("solve_nxn");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve x + y + z = 6; 2x - y + z = 3; -x + 2y + 3z = 12");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x1 = 1, x2 = 2, x3 = 3");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_nxn_linear_system_reports_inconsistent_system() {
        let bank_path = temp_bank_path("solve_nxn_inconsistent");
        let grammar_path = temp_grammar_path("solve_nxn_inconsistent");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve x + y + z = 6; 2x + 2y + 2z = 12; x + y + z = 7");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] no solution");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_nxn_linear_system_reports_infinite_solutions() {
        let bank_path = temp_bank_path("solve_nxn_infinite");
        let grammar_path = temp_grammar_path("solve_nxn_infinite");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve x + y + z = 6; 2x + 2y + 2z = 12; x - y + z = 2");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] infinitely many solutions");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_factorized_quadratic_form_returns_roots() {
        let bank_path = temp_bank_path("solve_factorized");
        let grammar_path = temp_grammar_path("solve_factorized");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve (x-2)(x-3)=0");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x1 = 2, x2 = 3");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_two_linear_equations_system_returns_xy() {
        let bank_path = temp_bank_path("solve_system");
        let grammar_path = temp_grammar_path("solve_system");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve 2x + y = 7; x - y = 2");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x = 3, y = 1");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_mode_includes_latex_steps_when_enabled() {
        let rendered = with_latex_enabled(|| {
            let solution = solve_equation("x^2 - 5x + 6 = 0");
            render_solve_equation("x^2 - 5x + 6 = 0", &solution)
        });

        assert!(rendered.contains("$$"));
        assert!(rendered.contains("x_1 = 2, x_2 = 3"));
        assert!(rendered.contains("\\left(-5\\right)^2"));
    }

    #[test]
    fn solve_linear_decimal_coefficients_show_exact_fraction_step() {
        let rendered = with_latex_enabled(|| {
            let solution = solve_equation("0.5x + 0.25 = 0");
            render_solve_equation("0.5x + 0.25 = 0", &solution)
        });

        assert!(rendered.contains("[SOLVE] x = -0.5"));
        assert!(rendered.contains("x = -\\frac{1}{2}"));
    }

    #[test]
    fn solve_system_decimal_coefficients_show_exact_cramers_steps() {
        let rendered = with_latex_enabled(|| {
            let solution = solve_equation("0.5x + y = 2; x - y = 1");
            render_solve_equation("0.5x + y = 2; x - y = 1", &solution)
        });

        assert!(rendered.contains("[SOLVE] x = 2, y = 1"));
        assert!(rendered.contains("\\Delta = -\\frac{3}{2}"));
        assert!(rendered.contains("x = 2"));
        assert!(rendered.contains("y = 1"));
    }

    #[test]
    fn solve_linear_inequality_returns_expected_bound() {
        let bank_path = temp_bank_path("solve_ineq");
        let grammar_path = temp_grammar_path("solve_ineq");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve 2x + 3 > 7");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in (2, +inf)");

        let flipped = agent.query("Solve -2x + 3 > 7");
        assert_eq!(flipped, "[CRYSTAL] [SOLVE] x in (-inf, -2)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_inequality_system_normalizes_intersection_interval() {
        let bank_path = temp_bank_path("solve_ineq_system");
        let grammar_path = temp_grammar_path("solve_ineq_system");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve x > 1; x <= 4");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in (1, 4]");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_chained_inequality_normalizes_interval() {
        let bank_path = temp_bank_path("solve_ineq_chain");
        let grammar_path = temp_grammar_path("solve_ineq_chain");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve 1 < x <= 3");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in (1, 3]");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_inequality_or_system_normalizes_union() {
        let bank_path = temp_bank_path("solve_ineq_union");
        let grammar_path = temp_grammar_path("solve_ineq_union");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve x < -1 or x >= 2");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in (-inf, -1) U [2, +inf)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_absolute_value_equation_returns_two_roots() {
        let bank_path = temp_bank_path("solve_abs_two");
        let grammar_path = temp_grammar_path("solve_abs_two");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve |x-3|=2");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x1 = 1, x2 = 5");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_absolute_value_negative_rhs_reports_no_solution() {
        let bank_path = temp_bank_path("solve_abs_none");
        let grammar_path = temp_grammar_path("solve_abs_none");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve |x-3|=-1");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] no solution");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_absolute_value_inequality_le_returns_single_interval() {
        let bank_path = temp_bank_path("solve_abs_ineq_le");
        let grammar_path = temp_grammar_path("solve_abs_ineq_le");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve |x-3| <= 2");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in [1, 5]");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_absolute_value_inequality_gt_returns_union() {
        let bank_path = temp_bank_path("solve_abs_ineq_gt");
        let grammar_path = temp_grammar_path("solve_abs_ineq_gt");
        let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path).unwrap();

        let answer = agent.query("Solve |x-3| > 2");
        assert_eq!(answer, "[CRYSTAL] [SOLVE] x in (-inf, 1) U (5, +inf)");

        let _ = fs::remove_file(bank_path);
        let _ = fs::remove_file(grammar_path);
    }

    #[test]
    fn solve_inequality_and_abs_include_latex_steps_when_enabled() {
        let (ineq, ineq_system, ineq_union, abs_ineq, abs) = with_latex_enabled(|| {
            (
                render_solve_equation("2x + 3 > 7", &solve_equation("2x + 3 > 7")),
                render_solve_equation("1 < x <= 3", &solve_equation("1 < x <= 3")),
                render_solve_equation("x < -1 or x >= 2", &solve_equation("x < -1 or x >= 2")),
                render_solve_equation("|x-3| > 2", &solve_equation("|x-3| > 2")),
                render_solve_equation("|x-3|=2", &solve_equation("|x-3|=2")),
            )
        });

        assert!(ineq.contains("$$"));
        assert!(ineq.contains("x \\in (2, +\\infty)"));
        assert!(ineq_system.contains("\\in (1, 3]"));
        assert!(ineq_union.contains("\\cup"));
        assert!(abs_ineq.contains("\\cup"));
        assert!(abs.contains("$$"));
        assert!(abs.contains("x_1 = 1, x_2 = 5"));
    }

    #[test]
    fn interval_set_normalization_merges_touching_bounds() {
        let a = Interval {
            lower: None,
            upper: Some((Rational::new(2, 1).unwrap(), true)),
            is_empty: false,
        };
        let b = Interval {
            lower: Some((Rational::new(2, 1).unwrap(), true)),
            upper: Some((Rational::new(5, 1).unwrap(), false)),
            is_empty: false,
        };

        let set = IntervalSet::from_intervals(vec![a, b]);
        assert_eq!(set.to_plain_union(), "(-inf, 5)");
    }

    #[test]
    fn interval_set_union_is_commutative_and_idempotent() {
        let a = IntervalSet::from_interval(Interval {
            lower: Some((Rational::new(1, 1).unwrap(), false)),
            upper: Some((Rational::new(4, 1).unwrap(), true)),
            is_empty: false,
        });
        let b = IntervalSet::from_interval(Interval {
            lower: Some((Rational::new(3, 1).unwrap(), false)),
            upper: None,
            is_empty: false,
        });

        let ab = a.union(&b).to_plain_union();
        let ba = b.union(&a).to_plain_union();
        let aa = a.union(&a).to_plain_union();

        assert_eq!(ab, ba);
        assert_eq!(aa, a.to_plain_union());
    }

    #[test]
    fn interval_set_intersection_distributes_over_union() {
        let a = IntervalSet::from_interval(Interval {
            lower: Some((Rational::new(0, 1).unwrap(), false)),
            upper: Some((Rational::new(10, 1).unwrap(), false)),
            is_empty: false,
        });
        let b = IntervalSet::from_interval(Interval {
            lower: None,
            upper: Some((Rational::new(3, 1).unwrap(), true)),
            is_empty: false,
        });
        let c = IntervalSet::from_interval(Interval {
            lower: Some((Rational::new(7, 1).unwrap(), true)),
            upper: None,
            is_empty: false,
        });

        let left = a.intersect(&b.union(&c)).to_plain_union();
        let right = a.intersect(&b).union(&a.intersect(&c)).to_plain_union();
        assert_eq!(left, right);
    }

    #[test]
    fn interval_set_difference_and_complement_render_holes() {
        let whole = IntervalSet::all_real();
        let holes = IntervalSet::from_intervals(vec![
            Interval {
                lower: Some((Rational::new(2, 1).unwrap(), true)),
                upper: Some((Rational::new(2, 1).unwrap(), true)),
                is_empty: false,
            },
            Interval {
                lower: Some((Rational::new(5, 1).unwrap(), true)),
                upper: Some((Rational::new(5, 1).unwrap(), true)),
                is_empty: false,
            },
        ]);

        assert_eq!(whole.difference(&holes).to_plain_union(), "(-inf, +inf) \\ {2, 5}");
        assert_eq!(holes.complement().to_plain_union(), "(-inf, +inf) \\ {2, 5}");
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
