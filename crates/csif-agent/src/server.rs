use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{get, post},
    Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use crate::agent::{
    evaluate_instruction_execution, verify_proof_certificate, AntiLobeSnapshot, CSIFAgent,
    CompositeQuerySummary, ExplainResultPayload, InstructionExecutionDecision, ProofCertificate,
    QueryClauseResult, RequestTimeContext, RouteAuditTrail, PlayAttempt, PlayAttemptOutcome,
    current_request_time_context_with_initiator,
};
use crate::metadata::AppliedLobe;
use tokio_stream::iter;
use tokio::time::Duration;

#[derive(Debug, Deserialize)]
struct QueryRequest {
    text: String,
}

#[derive(Debug, Deserialize)]
struct ExplainRequest {
    text: String,
}

#[derive(Debug, Deserialize)]
struct VisualizeRequest {
    text: String,
    #[serde(default)]
    format: Option<String>,
}

#[derive(Debug, Serialize)]
struct QueryResponse {
    answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    certificate: Option<ProofCertificate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clauses: Option<Vec<QueryClauseResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    composite: Option<CompositeQuerySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_time_context: Option<RequestTimeContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_audit: Option<RouteAuditTrail>,
}

#[derive(Debug, Serialize)]
struct VisualizeResponse {
    format: String,
    mime_type: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct VerifyProofRequest {
    certificate: ProofCertificate,
}

#[derive(Debug, Serialize)]
struct VerifyProofResponse {
    ok: bool,
    family: String,
}

#[derive(Debug, Deserialize)]
struct ExecutePlanRequest {
    certificate: ProofCertificate,
    action_index: usize,
    #[serde(default)]
    approval_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExecuteAuditEvent {
    timestamp: String,
    certificate_family: String,
    action_index: usize,
    action_hash: String,
    ok: bool,
    executed: bool,
    requires_approval: bool,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct AdminExecuteAuditQuery {
    limit: Option<usize>,
    family: Option<String>,
    action_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AdminLoopQuery {
    force: Option<String>,
}

#[derive(Debug, Serialize)]
struct AdminExecuteAuditResponse {
    path: Option<String>,
    limit: usize,
    returned: usize,
    events: Vec<ExecuteAuditEvent>,
}

#[derive(Debug, Serialize)]
struct AdminLobesResponse {
    lobe_dir: Option<String>,
    poll_secs: Option<u64>,
    strict_mode: bool,
    applied: Vec<AppliedLobe>,
}

#[derive(Debug, Serialize)]
struct AdminLobesReloadResponse {
    lobe_dir: Option<String>,
    report: crate::agent::LobeRefreshReport,
    applied: Vec<AppliedLobe>,
}

#[derive(Debug, Serialize)]
struct AdminAntiLobeResponse {
    anti_lobe: AntiLobeSnapshot,
}

#[derive(Debug, Clone, Serialize)]
struct LoopAuditRecord {
    timestamp: String,
    initiator: String,
    query: String,
    route_audit: RouteAuditTrail,
    stop_reason: String,
    request_time_context: RequestTimeContext,
    taught: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anomaly_classification: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anomaly_score: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
struct PlaySchedulerStatus {
    enabled: bool,
    poll_secs: u64,
    max_cycles_per_tick: usize,
    max_ms: u64,
    write_approval_configured: bool,
    history_limit: usize,
    stored_cycles: usize,
}

#[derive(Debug, Clone, Serialize)]
struct PlayCycleRecord {
    started_at: String,
    completed_at: String,
    cycles_run: usize,
    attempts: Vec<PlayAttempt>,
    audit_events: Vec<LoopAuditRecord>,
    success_crystallized: usize,
    failure_persisted: usize,
    suppressed_failures: usize,
}

#[derive(Debug, Serialize)]
struct AdminPlayResponse {
    scheduler: PlaySchedulerStatus,
    recent_cycles: Vec<PlayCycleRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct ObservationSchedulerStatus {
    enabled: bool,
    poll_secs: u64,
    max_ms: u64,
    history_limit: usize,
    source: String,
    query: String,
    write_approval_configured: bool,
    no_supporting_path_threshold: usize,
    contradiction_threshold: usize,
    stored_cycles: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ObservationCycleRecord {
    started_at: String,
    completed_at: String,
    observations_run: usize,
    anomalies_detected: usize,
    audit_events: Vec<LoopAuditRecord>,
}

#[derive(Debug, Serialize)]
struct AdminObservationResponse {
    scheduler: ObservationSchedulerStatus,
    recent_cycles: Vec<ObservationCycleRecord>,
}

#[derive(Debug, Clone)]
struct PlaySchedulerConfig {
    enabled: bool,
    poll_secs: u64,
    max_cycles_per_tick: usize,
    max_ms: u64,
    history_limit: usize,
    approval_token: Option<String>,
}

#[derive(Debug, Clone)]
struct ObservationSchedulerConfig {
    enabled: bool,
    poll_secs: u64,
    max_ms: u64,
    history_limit: usize,
    source: String,
    query: String,
    approval_token: Option<String>,
    anomaly_policy: ObservationAnomalyPolicy,
}

#[derive(Debug, Clone)]
struct ObservationAnomalyPolicy {
    no_supporting_path_threshold: usize,
    contradiction_threshold: usize,
}

#[derive(Debug)]
struct PlayRuntimeState {
    config: PlaySchedulerConfig,
    history: Mutex<VecDeque<PlayCycleRecord>>,
}

#[derive(Debug)]
struct ObservationRuntimeState {
    config: ObservationSchedulerConfig,
    history: Mutex<VecDeque<ObservationCycleRecord>>,
}

struct AppState {
    agent: Arc<Mutex<CSIFAgent>>,
    play_runtime: Arc<PlayRuntimeState>,
    observation_runtime: Arc<ObservationRuntimeState>,
}

impl AppState {
    fn new(agent: Arc<Mutex<CSIFAgent>>) -> Self {
        Self {
            agent,
            play_runtime: Arc::new(PlayRuntimeState {
                config: play_scheduler_config_from_env(),
                history: Mutex::new(VecDeque::new()),
            }),
            observation_runtime: Arc::new(ObservationRuntimeState {
                config: observation_scheduler_config_from_env(),
                history: Mutex::new(VecDeque::new()),
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: Option<String>,
    messages: Vec<ChatMessage>,
    stream: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    role: String,
    content: ChatMessageContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
    SinglePart(ChatContentPart),
}

#[derive(Debug, Deserialize)]
struct ChatContentPart {
    #[serde(rename = "type")]
    part_type: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Serialize)]
struct ChatChoice {
    index: u32,
    message: ChatResponseMessage,
    finish_reason: String,
}

#[derive(Debug, Serialize)]
struct ChatResponseMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionChunkResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<ChatChunkChoice>,
}

#[derive(Debug, Serialize)]
struct ChatChunkChoice {
    index: u32,
    delta: ChatDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Default)]
struct ChatDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

#[derive(Debug, Serialize)]
struct ModelsResponse {
    object: String,
    data: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
struct ModelInfo {
    id: String,
    object: String,
    created: u64,
    owned_by: String,
    root: String,
    parent: Option<String>,
    permission: Vec<ModelPermission>,
}

#[derive(Debug, Serialize)]
struct ModelPermission {
    id: String,
    object: String,
    created: u64,
    allow_create_engine: bool,
    allow_sampling: bool,
    allow_logprobs: bool,
    allow_search_indices: bool,
    allow_view: bool,
    allow_fine_tuning: bool,
    organization: String,
    group: Option<String>,
    is_blocking: bool,
}

static CHAT_COMPLETION_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn build_app(agent: Arc<Mutex<CSIFAgent>>) -> Router {
    let state = Arc::new(AppState::new(agent));

    build_router(state)
}

fn build_router(state: Arc<AppState>) -> Router {

    Router::new()
        .route("/health", get(health_handler))
        .route("/query", post(query_handler))
        .route("/explain", post(explain_handler))
        .route("/visualize", post(visualize_handler))
        .route("/teach", post(teach_handler))
        .route("/verify-proof", post(verify_proof_handler))
        .route("/execute-plan", post(execute_plan_handler))
        .route("/admin/lobes", get(admin_lobes_handler))
        .route("/admin/lobes/reload", post(admin_lobes_reload_handler))
        .route("/admin/anti-lobe", get(admin_anti_lobe_handler))
        .route("/admin/play", get(admin_play_handler))
        .route("/admin/observation", get(admin_observation_handler))
        .route("/admin/execute-audit", get(admin_execute_audit_handler))
        .route("/v1/chat/completions", post(chat_completion_handler))
        .route("/v1/models", get(models_handler))
        .route("/v1/models/:model_id", get(model_handler))
        .with_state(state)
}

pub async fn start_server(agent: Arc<Mutex<CSIFAgent>>, port: u16) {
    let state = Arc::new(AppState::new(agent));
    spawn_play_scheduler(Arc::clone(&state));
    spawn_observation_scheduler(Arc::clone(&state));

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    println!("CSIF Agent listening on port {}", port);
    axum::serve(listener, app).await.unwrap();
}

async fn query_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Json<QueryResponse> {
    let agent = &state.agent;
    let mut agent = agent.lock().unwrap();
    let result = agent.query_with_certificate(&req.text);
    Json(QueryResponse {
        answer: result.answer,
        certificate: result.certificate,
        clauses: result.clauses,
        composite: result.composite,
        request_time_context: result.request_time_context,
        route_audit: result.route_audit,
    })
}

async fn teach_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Json<QueryResponse> {
    let agent = &state.agent;
    let mut agent = agent.lock().unwrap();
    let answer = agent.teach(&req.text);
    Json(QueryResponse {
        answer,
        certificate: None,
        clauses: None,
        composite: None,
        request_time_context: None,
        route_audit: None,
    })
}

async fn explain_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExplainRequest>,
) -> Json<ExplainResultPayload> {
    let agent = &state.agent;
    let agent = agent.lock().unwrap();
    Json(agent.explain_query(&req.text))
}

async fn visualize_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VisualizeRequest>,
) -> Json<VisualizeResponse> {
    let format = req
        .format
        .as_deref()
        .unwrap_or("dot")
        .trim()
        .to_lowercase();

    let agent = &state.agent;
    let mut agent = agent.lock().unwrap();
    let explain = agent.explain_query(&req.text);
    let query = agent.query_with_certificate(&req.text);

    let (mime_type, content) = match format.as_str() {
        "tree" => (
            "text/plain".to_string(),
            render_visualization_tree(&req.text, &explain, &query.answer),
        ),
        "latex" => (
            "text/plain".to_string(),
            render_visualization_latex(&query.answer),
        ),
        _ => (
            "text/vnd.graphviz".to_string(),
            render_visualization_dot(&req.text, &explain),
        ),
    };

    Json(VisualizeResponse {
        format,
        mime_type,
        content,
    })
}

async fn verify_proof_handler(Json(req): Json<VerifyProofRequest>) -> Json<VerifyProofResponse> {
    let family = req.certificate.family().to_string();
    let ok = verify_proof_certificate(&req.certificate);
    Json(VerifyProofResponse { ok, family })
}

async fn execute_plan_handler(
    Json(req): Json<ExecutePlanRequest>,
) -> Json<InstructionExecutionDecision> {
    let certificate_family = req.certificate.family().to_string();
    let action_index = req.action_index;
    let decision = evaluate_instruction_execution(
        &req.certificate,
        action_index,
        req.approval_token.as_deref(),
    );
    let audit_event = ExecuteAuditEvent {
        timestamp: Utc::now().to_rfc3339(),
        certificate_family,
        action_index,
        action_hash: hash_action_signature(
            action_index,
            decision.action_kind.as_deref(),
            decision.action_command.as_deref(),
        ),
        ok: decision.ok,
        executed: decision.executed,
        requires_approval: decision.requires_approval,
        reason: decision.reason.clone(),
    };
    append_execute_audit_event(&audit_event);
    Json(decision)
}

fn hash_action_signature(action_index: usize, kind: Option<&str>, command: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(action_index.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(kind.unwrap_or("<none>").as_bytes());
    hasher.update(b"|");
    hasher.update(command.unwrap_or("<none>").as_bytes());
    format!("{:x}", hasher.finalize())
}

fn append_execute_audit_event(event: &ExecuteAuditEvent) {
    let payload = match serde_json::to_string(event) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("execute-audit serialization failed: {err}");
            return;
        }
    };

    if let Ok(path_value) = std::env::var("CSIF_EXEC_AUDIT_LOG_PATH") {
        let path = Path::new(&path_value);
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                eprintln!("execute-audit mkdir failed: {err}");
                return;
            }
        }
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(mut file) => {
                if let Err(err) = writeln!(file, "{payload}") {
                    eprintln!("execute-audit write failed: {err}");
                }
            }
            Err(err) => eprintln!("execute-audit open failed: {err}"),
        }
    } else {
        eprintln!("execute-audit {payload}");
    }
}

async fn admin_execute_audit_handler(
    headers: HeaderMap,
    Query(query): Query<AdminExecuteAuditQuery>,
) -> Result<Json<AdminExecuteAuditResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin_token(&headers)?;

    let limit = query.limit.unwrap_or(50).min(1000);
    let family_filter = query.family.as_deref();
    let action_hash_filter = query.action_hash.as_deref();

    let path_value = std::env::var("CSIF_EXEC_AUDIT_LOG_PATH").ok();
    let Some(path_value) = path_value else {
        return Ok(Json(AdminExecuteAuditResponse {
            path: None,
            limit,
            returned: 0,
            events: Vec::new(),
        }));
    };

    let path = Path::new(&path_value);
    if !path.exists() {
        return Ok(Json(AdminExecuteAuditResponse {
            path: Some(path_value),
            limit,
            returned: 0,
            events: Vec::new(),
        }));
    }

    let file = std::fs::File::open(path).map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": {
                    "message": format!("failed to open execute audit log: {err}"),
                    "type": "internal_error"
                }
            })),
        )
    })?;

    let mut events = Vec::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<ExecuteAuditEvent>(&line) else {
            continue;
        };
        if let Some(family) = family_filter {
            if event.certificate_family != family {
                continue;
            }
        }
        if let Some(action_hash) = action_hash_filter {
            if event.action_hash != action_hash {
                continue;
            }
        }
        events.push(event);
    }

    let returned = events.len().min(limit);
    let events = events.into_iter().rev().take(limit).collect::<Vec<_>>();

    Ok(Json(AdminExecuteAuditResponse {
        path: Some(path_value),
        limit,
        returned,
        events,
    }))
}

async fn admin_lobes_handler(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<AdminLobesResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin_token(&headers)?;

    let agent = &state.agent;

    let strict_mode = std::env::var("CSIF_LOBES_STRICT")
        .ok()
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);
    let lobe_dir = std::env::var("CSIF_LOBES_DIR").ok();
    let poll_secs = std::env::var("CSIF_LOBES_POLL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());

    let applied = {
        let agent = agent.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "failed to acquire agent lock",
                        "type": "internal_error"
                    }
                })),
            )
        })?;
        agent.applied_lobes().map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": format!("failed to load lobe state: {err}"),
                        "type": "internal_error"
                    }
                })),
            )
        })?
    };

    Ok(Json(AdminLobesResponse {
        lobe_dir,
        poll_secs,
        strict_mode,
        applied,
    }))
}

async fn admin_lobes_reload_handler(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<AdminLobesReloadResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin_token(&headers)?;

    let agent = &state.agent;

    let lobe_dir = std::env::var("CSIF_LOBES_DIR").ok();

    let (report, applied) = {
        let mut agent = agent.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "failed to acquire agent lock",
                        "type": "internal_error"
                    }
                })),
            )
        })?;

        let report = agent.refresh_lobes_from_env().map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": format!("failed to refresh lobes: {err}"),
                        "type": "internal_error"
                    }
                })),
            )
        })?;

        let applied = agent.applied_lobes().map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": format!("failed to load lobe state: {err}"),
                        "type": "internal_error"
                    }
                })),
            )
        })?;

        (report, applied)
    };

    Ok(Json(AdminLobesReloadResponse {
        lobe_dir,
        report,
        applied,
    }))
}

async fn admin_anti_lobe_handler(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<AdminAntiLobeResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin_token(&headers)?;

    let agent = &state.agent;

    let anti_lobe = {
        let agent = agent.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "failed to acquire agent lock",
                        "type": "internal_error"
                    }
                })),
            )
        })?;
        agent.anti_lobe_snapshot()
    };

    Ok(Json(AdminAntiLobeResponse { anti_lobe }))
}

async fn admin_play_handler(
    headers: HeaderMap,
    Query(query): Query<AdminLoopQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<AdminPlayResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin_token(&headers)?;

    if parse_force_flag(query.force.as_deref()) {
        // Force ticks are potentially expensive and use blocking primitives.
        // Run them on the blocking pool with a timeout so the async runtime
        // remains responsive even if a force tick stalls.
        let force_timeout = Duration::from_millis(
            (state.play_runtime.config.max_ms as u64)
                .saturating_mul(6)
                .saturating_add(500),
        );
        let state_for_tick = Arc::clone(&state);
        let forced_tick = tokio::time::timeout(force_timeout, tokio::task::spawn_blocking(move || {
            run_bounded_play_tick(&state_for_tick)
        }))
        .await;

        if let Ok(Ok(Some(record))) = forced_tick {
            push_play_history(&state, record);
        }
    }

    let scheduler = {
        let history = state.play_runtime.history.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "failed to acquire play runtime lock",
                        "type": "internal_error"
                    }
                })),
            )
        })?;

        PlaySchedulerStatus {
            enabled: state.play_runtime.config.enabled,
            poll_secs: state.play_runtime.config.poll_secs,
            max_cycles_per_tick: state.play_runtime.config.max_cycles_per_tick,
            max_ms: state.play_runtime.config.max_ms,
            write_approval_configured: state.play_runtime.config.approval_token.is_some(),
            history_limit: state.play_runtime.config.history_limit,
            stored_cycles: history.len(),
        }
    };

    let recent_cycles = {
        let history = state.play_runtime.history.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "failed to acquire play runtime lock",
                        "type": "internal_error"
                    }
                })),
            )
        })?;
        history.iter().rev().cloned().collect::<Vec<_>>()
    };

    Ok(Json(AdminPlayResponse {
        scheduler,
        recent_cycles,
    }))
}

async fn admin_observation_handler(
    headers: HeaderMap,
    Query(query): Query<AdminLoopQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<AdminObservationResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin_token(&headers)?;

    if parse_force_flag(query.force.as_deref()) {
        let force_timeout = Duration::from_millis(
            (state.observation_runtime.config.max_ms as u64)
                .saturating_mul(6)
                .saturating_add(500),
        );
        let state_for_tick = Arc::clone(&state);
        let forced_tick = tokio::time::timeout(force_timeout, tokio::task::spawn_blocking(move || {
            run_observation_tick(&state_for_tick)
        }))
        .await;

        if let Ok(Ok(Some(record))) = forced_tick {
            push_observation_history(&state, record);
        }
    }

    let scheduler = {
        let history = state.observation_runtime.history.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "failed to acquire observation runtime lock",
                        "type": "internal_error"
                    }
                })),
            )
        })?;

        ObservationSchedulerStatus {
            enabled: state.observation_runtime.config.enabled,
            poll_secs: state.observation_runtime.config.poll_secs,
            max_ms: state.observation_runtime.config.max_ms,
            history_limit: state.observation_runtime.config.history_limit,
            source: state.observation_runtime.config.source.clone(),
            query: state.observation_runtime.config.query.clone(),
            write_approval_configured: state.observation_runtime.config.approval_token.is_some(),
            no_supporting_path_threshold: state
                .observation_runtime
                .config
                .anomaly_policy
                .no_supporting_path_threshold,
            contradiction_threshold: state
                .observation_runtime
                .config
                .anomaly_policy
                .contradiction_threshold,
            stored_cycles: history.len(),
        }
    };

    let recent_cycles = {
        let history = state.observation_runtime.history.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": {
                        "message": "failed to acquire observation runtime lock",
                        "type": "internal_error"
                    }
                })),
            )
        })?;
        history.iter().rev().cloned().collect::<Vec<_>>()
    };

    Ok(Json(AdminObservationResponse {
        scheduler,
        recent_cycles,
    }))
}

fn require_admin_token(
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let Some(expected_token) = std::env::var("CSIF_ADMIN_TOKEN").ok().filter(|value| !value.is_empty()) else {
        return Ok(());
    };

    let provided_token = headers
        .get("x-csif-admin-token")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .map(str::to_string)
        });

    match provided_token {
        Some(token) if token == expected_token => Ok(()),
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": {
                    "message": "missing or invalid admin token",
                    "type": "unauthorized"
                }
            })),
        )),
    }
}

async fn chat_completion_handler(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let _ = headers.get("authorization");
    let stream = req.stream.unwrap_or(false);

    let user_message = req
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| extract_message_text(&message.content))
        .unwrap_or_default();

    let agent = &state.agent;
    let mut agent = agent.lock().unwrap();
    let answer = agent.query(&user_message);

    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let sequence = CHAT_COMPLETION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let model = req.model.unwrap_or_else(|| "csif-agent".to_string());

    let response = ChatCompletionResponse {
        id: format!("chatcmpl-{}-{}", created, sequence),
        object: "chat.completion".to_string(),
        created,
        model,
        choices: vec![ChatChoice {
            index: 0,
            message: ChatResponseMessage {
                role: "assistant".to_string(),
                content: answer.clone(),
            },
            finish_reason: "stop".to_string(),
        }],
    };

    if stream {
        let first_chunk = ChatCompletionChunkResponse {
            id: response.id.clone(),
            object: "chat.completion.chunk".to_string(),
            created: response.created,
            model: response.model.clone(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta: ChatDelta {
                    role: Some("assistant".to_string()),
                    content: Some(answer),
                },
                finish_reason: None,
            }],
        };
        let final_chunk = ChatCompletionChunkResponse {
            id: response.id.clone(),
            object: "chat.completion.chunk".to_string(),
            created: response.created,
            model: response.model.clone(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta: ChatDelta::default(),
                finish_reason: Some("stop".to_string()),
            }],
        };

        let first_payload = serde_json::to_string(&first_chunk).unwrap_or_else(|_| "{}".to_string());
        let final_payload = serde_json::to_string(&final_chunk).unwrap_or_else(|_| "{}".to_string());
        let event_stream = iter(vec![
            Ok::<Event, Infallible>(Event::default().data(first_payload)),
            Ok::<Event, Infallible>(Event::default().data(final_payload)),
            Ok::<Event, Infallible>(Event::default().data("[DONE]")),
        ]);

        Ok(Sse::new(event_stream)
            .keep_alive(KeepAlive::default())
            .into_response())
    } else {
        Ok(Json(response).into_response())
    }
}

async fn models_handler() -> Json<ModelsResponse> {
    let model = csif_model_info();
    Json(ModelsResponse {
        object: "list".to_string(),
        data: vec![model],
    })
}

async fn model_handler(
    axum::extract::Path(model_id): axum::extract::Path<String>,
) -> Result<Json<ModelInfo>, (StatusCode, Json<serde_json::Value>)> {
    let model = csif_model_info();
    if model.id == model_id {
        Ok(Json(model))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "message": format!("model '{}' not found", model_id),
                    "type": "invalid_request_error",
                    "param": "model_id",
                    "code": "model_not_found"
                }
            })),
        ))
    }
}

fn csif_model_info() -> ModelInfo {
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();

    ModelInfo {
        id: "csif-agent".to_string(),
        object: "model".to_string(),
        created,
        owned_by: "motechnicalities".to_string(),
        root: "csif-agent".to_string(),
        parent: None,
        permission: vec![ModelPermission {
            id: format!("modelperm-{}", created),
            object: "model_permission".to_string(),
            created,
            allow_create_engine: false,
            allow_sampling: true,
            allow_logprobs: false,
            allow_search_indices: false,
            allow_view: true,
            allow_fine_tuning: false,
            organization: "*".to_string(),
            group: None,
            is_blocking: false,
        }],
    }
}

async fn health_handler() -> &'static str {
    "ok"
}

fn extract_message_text(content: &ChatMessageContent) -> String {
    match content {
        ChatMessageContent::Text(text) => text.clone(),
        ChatMessageContent::SinglePart(part) => part.text.clone().unwrap_or_default(),
        ChatMessageContent::Parts(parts) => parts
            .iter()
            .filter(|part| {
                let t = part.part_type.as_deref().unwrap_or("text");
                t == "text" || t == "input_text"
            })
            .filter_map(|part| part.text.clone())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn render_visualization_dot(input: &str, explain: &ExplainResultPayload) -> String {
    let mut lines = vec![
        "digraph csif_proof {".to_string(),
        "  rankdir=LR;".to_string(),
        "  node [shape=box, style=rounded];".to_string(),
        format!("  query [label=\"query: {}\"];", input.replace('"', "\\\"")),
    ];

    if explain.path.is_empty() {
        lines.push("  result [label=\"no relation path\"];".to_string());
        lines.push("  query -> result [label=\"unresolved\"];".to_string());
    } else {
        for (idx, node) in explain.path.iter().enumerate() {
            lines.push(format!(
                "  n{} [label=\"{}\"];",
                idx,
                node.replace('"', "\\\"")
            ));
            if idx == 0 {
                lines.push("  query -> n0 [label=\"start\"];".to_string());
            }
        }
        let relation = explain.relation.as_deref().unwrap_or("related_to");
        for idx in 0..explain.path.len().saturating_sub(1) {
            lines.push(format!("  n{} -> n{} [label=\"{}\"];", idx, idx + 1, relation));
        }
    }

    lines.push("}".to_string());
    lines.join("\n")
}

fn render_visualization_tree(input: &str, explain: &ExplainResultPayload, answer: &str) -> String {
    let mut lines = vec![
        "Proof Tree".to_string(),
        format!("- query: {}", input),
        format!("- intent: {}", explain.intent),
        format!("- answer: {}", answer),
    ];

    if let Some(relation) = &explain.relation {
        lines.push(format!("- relation: {}", relation));
    }
    if let Some(limit) = explain.depth_limit {
        lines.push(format!("- depth_limit: {}", limit));
    }
    if let Some(confidence) = explain.confidence {
        lines.push(format!("- confidence: {:.3}", confidence));
    }

    lines.push("- path:".to_string());
    if explain.path.is_empty() {
        lines.push("  - (none)".to_string());
    } else {
        for node in &explain.path {
            lines.push(format!("  - {}", node));
        }
    }

    lines.push("- contradiction_review:".to_string());
    for note in &explain.considered_contradictions {
        lines.push(format!("  - {}", note));
    }

    lines.join("\n")
}

fn render_visualization_latex(answer: &str) -> String {
    let latex_lines = answer
        .lines()
        .filter(|line| line.trim_start().starts_with("$$"))
        .collect::<Vec<_>>();

    if latex_lines.is_empty() {
        format!(
            "\\text{{No LaTeX scaffold in answer.}}\\\\\\n\\text{{Answer: {}}}",
            answer.replace('{', "\\{").replace('}', "\\}")
        )
    } else {
        latex_lines.join("\n")
    }
}

fn play_scheduler_config_from_env() -> PlaySchedulerConfig {
    let enabled = env_bool("CSIF_PLAY_ENABLED", false);
    let poll_secs = std::env::var("CSIF_PLAY_POLL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30)
        .max(1);
    let max_cycles_per_tick = std::env::var("CSIF_PLAY_MAX_CYCLES_PER_TICK")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2)
        .clamp(1, 32);
    let history_limit = std::env::var("CSIF_PLAY_HISTORY_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, 1000);
    let max_ms = std::env::var("CSIF_PLAY_MAX_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(200)
        .max(1);
    let approval_token = std::env::var("CSIF_PLAY_APPROVAL_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());

    PlaySchedulerConfig {
        enabled,
        poll_secs,
        max_cycles_per_tick,
        max_ms,
        history_limit,
        approval_token,
    }
}

fn observation_scheduler_config_from_env() -> ObservationSchedulerConfig {
    let enabled = env_bool("CSIF_OBSERVE_ENABLED", false);
    let poll_secs = std::env::var("CSIF_OBSERVE_POLL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3600)
        .max(1);
    let max_ms = std::env::var("CSIF_OBSERVE_MAX_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(250)
        .max(1);
    let history_limit = std::env::var("CSIF_OBSERVE_HISTORY_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, 1000);
    let source = std::env::var("CSIF_OBSERVE_SOURCE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "internal:observation".to_string());
    let query = std::env::var("CSIF_OBSERVE_QUERY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Is a whale a reptile?".to_string());
    let approval_token = std::env::var("CSIF_OBSERVE_APPROVAL_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let anomaly_policy = ObservationAnomalyPolicy {
        no_supporting_path_threshold: std::env::var("CSIF_OBSERVE_NO_PATH_THRESHOLD")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1)
            .max(1),
        contradiction_threshold: std::env::var("CSIF_OBSERVE_CONTRADICTION_THRESHOLD")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1)
            .max(1),
    };

    ObservationSchedulerConfig {
        enabled,
        poll_secs,
        max_ms,
        history_limit,
        source,
        query,
        approval_token,
        anomaly_policy,
    }
}

fn spawn_play_scheduler(state: Arc<AppState>) {
    if !state.play_runtime.config.enabled {
        return;
    }

    let poll_secs = state.play_runtime.config.poll_secs;
    let state_for_task = Arc::clone(&state);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(poll_secs));
        loop {
            ticker.tick().await;
            let record = run_bounded_play_tick(&state_for_task);
            if let Some(record) = record {
                push_play_history(&state_for_task, record);
            }
        }
    });
}

fn spawn_observation_scheduler(state: Arc<AppState>) {
    if !state.observation_runtime.config.enabled {
        return;
    }

    let poll_secs = state.observation_runtime.config.poll_secs;
    let state_for_task = Arc::clone(&state);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(poll_secs));
        loop {
            ticker.tick().await;
            let record = run_observation_tick(&state_for_task);
            if let Some(record) = record {
                push_observation_history(&state_for_task, record);
            }
        }
    });
}

fn run_bounded_play_tick(state: &AppState) -> Option<PlayCycleRecord> {
    let started_at = Utc::now();
    let tick_started = std::time::Instant::now();
    let mut attempts = Vec::new();
    let mut audit_events = Vec::new();
    let mut cycles_run = 0usize;
    let max_cycles_per_tick = state.play_runtime.config.max_cycles_per_tick;
    let max_ms = state.play_runtime.config.max_ms;
    let writes_enabled = state.play_runtime.config.approval_token.is_some();

    for _ in 0..max_cycles_per_tick {
        if tick_started.elapsed().as_millis() >= max_ms as u128 {
            break;
        }

        let (mut cycle_attempts, anti_lobe_bank_path) = {
            // Avoid blocking the async runtime on force-triggered ticks when the
            // scheduler or another request already holds the agent lock.
            let mut guard = match state.agent.try_lock() {
                Ok(guard) => guard,
                Err(_) => break,
            };
            let anti_lobe_bank_path = guard.anti_lobe_bank_path.display().to_string();
            let cycle_attempts = if writes_enabled {
                guard.run_play_cycle()
            } else {
                guard.preview_play_cycle()
            };
            (cycle_attempts, Some(anti_lobe_bank_path))
        };

        if cycle_attempts.is_empty() {
            break;
        }

        if !writes_enabled {
            for attempt in &mut cycle_attempts {
                if matches!(
                    attempt.outcome,
                    PlayAttemptOutcome::SuccessCrystallized | PlayAttemptOutcome::FailurePersisted
                ) {
                    attempt.detail = format!(
                        "{} (write skipped: missing CSIF_PLAY_APPROVAL_TOKEN)",
                        attempt.detail
                    );
                }
            }
        }

        for attempt in &cycle_attempts {
            let query = play_query_for_attempt(attempt);
            let stop_reason = play_stop_reason_for_attempt(attempt.outcome.clone());
            let context = current_request_time_context_with_initiator("system:play");
            let route_audit = RouteAuditTrail {
                relation: Some(attempt.relation.clone()),
                subject: Some(attempt.subject.clone()),
                object: Some(attempt.object.clone()),
                tried: attempt.basis.clone(),
                stop_reason: stop_reason.clone(),
                negative_evidence: vec![attempt.detail.clone()],
                anti_lobe_bank_path: match attempt.outcome {
                    PlayAttemptOutcome::SkippedKnownFailure | PlayAttemptOutcome::FailurePersisted => {
                        anti_lobe_bank_path.clone()
                    }
                    _ => None,
                },
            };
            let taught = writes_enabled
                && matches!(
                    attempt.outcome,
                    PlayAttemptOutcome::SuccessCrystallized | PlayAttemptOutcome::FailurePersisted
                );
            let event = LoopAuditRecord {
                timestamp: context.request_received_at.clone(),
                initiator: "system:play".to_string(),
                query,
                route_audit,
                stop_reason,
                request_time_context: context,
                taught,
                source: Some("internal:play".to_string()),
                anomaly_classification: None,
                anomaly_score: None,
            };
            append_loop_audit_event(&event);
            audit_events.push(event);
        }

        let made_progress = cycle_attempts.iter().any(|attempt| {
            matches!(
                attempt.outcome,
                PlayAttemptOutcome::SuccessCrystallized | PlayAttemptOutcome::FailurePersisted
            )
        });

        attempts.extend(cycle_attempts);
        cycles_run += 1;

        if !made_progress {
            break;
        }
    }

    if attempts.is_empty() && audit_events.is_empty() {
        return None;
    }

    let mut success_crystallized = 0usize;
    let mut failure_persisted = 0usize;
    let mut suppressed_failures = 0usize;
    for attempt in &attempts {
        match attempt.outcome {
            PlayAttemptOutcome::SuccessCrystallized => success_crystallized += 1,
            PlayAttemptOutcome::FailurePersisted => failure_persisted += 1,
            PlayAttemptOutcome::SkippedKnownFailure => suppressed_failures += 1,
            PlayAttemptOutcome::SkippedKnownSuccess => {}
        }
    }

    Some(PlayCycleRecord {
        started_at: started_at.to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        cycles_run,
        attempts,
        audit_events,
        success_crystallized,
        failure_persisted,
        suppressed_failures,
    })
}

fn run_observation_tick(state: &AppState) -> Option<ObservationCycleRecord> {
    let started_at = Utc::now();
    let tick_started = std::time::Instant::now();
    let max_ms = state.observation_runtime.config.max_ms;
    if tick_started.elapsed().as_millis() >= max_ms as u128 {
        return None;
    }

    let source = state.observation_runtime.config.source.clone();
    let query = state.observation_runtime.config.query.clone();

    let result = {
        let mut guard = match state.agent.lock() {
            Ok(guard) => guard,
            Err(_) => return None,
        };
        guard.query_with_certificate(&query)
    };

    let mut context = result
        .request_time_context
        .unwrap_or_else(|| current_request_time_context_with_initiator("system:observation"));
    context.initiator = "system:observation".to_string();

    let route_audit = result.route_audit.unwrap_or(RouteAuditTrail {
        relation: None,
        subject: None,
        object: None,
        tried: Vec::new(),
        stop_reason: "no_route_audit_available".to_string(),
        negative_evidence: vec!["Query completed without relation route audit.".to_string()],
        anti_lobe_bank_path: None,
    });
    let stop_reason = route_audit.stop_reason.clone();
    let anomaly = classify_observation_anomaly(
        &route_audit,
        &state.observation_runtime.config.anomaly_policy,
    );
    let event = LoopAuditRecord {
        timestamp: context.request_received_at.clone(),
        initiator: "system:observation".to_string(),
        query,
        route_audit,
        stop_reason,
        request_time_context: context,
        taught: false,
        source: Some(source),
        anomaly_classification: anomaly.as_ref().map(|result| result.classification.clone()),
        anomaly_score: anomaly.as_ref().map(|result| result.score),
    };
    append_loop_audit_event(&event);

    Some(ObservationCycleRecord {
        started_at: started_at.to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        observations_run: 1,
        anomalies_detected: usize::from(anomaly.is_some()),
        audit_events: vec![event],
    })
}

#[derive(Debug, Clone)]
struct ObservationAnomalyResult {
    classification: String,
    score: usize,
}

fn classify_observation_anomaly(
    route_audit: &RouteAuditTrail,
    policy: &ObservationAnomalyPolicy,
) -> Option<ObservationAnomalyResult> {
    let score = route_audit.negative_evidence.len().max(1);
    if route_audit.stop_reason.contains("contradiction") && score >= policy.contradiction_threshold {
        return Some(ObservationAnomalyResult {
            classification: "contradiction".to_string(),
            score,
        });
    }
    if route_audit.stop_reason.contains("no_supporting_path")
        && score >= policy.no_supporting_path_threshold
    {
        return Some(ObservationAnomalyResult {
            classification: "no_supporting_path".to_string(),
            score,
        });
    }
    None
}

fn push_play_history(state: &AppState, record: PlayCycleRecord) {
    if let Ok(mut history) = state.play_runtime.history.lock() {
        history.push_back(record);
        while history.len() > state.play_runtime.config.history_limit {
            history.pop_front();
        }
    }
}

fn push_observation_history(state: &AppState, record: ObservationCycleRecord) {
    if let Ok(mut history) = state.observation_runtime.history.lock() {
        history.push_back(record);
        while history.len() > state.observation_runtime.config.history_limit {
            history.pop_front();
        }
    }
}

fn play_query_for_attempt(attempt: &PlayAttempt) -> String {
    match attempt.relation.as_str() {
        "is_a" => format!("Is a {} a {}?", attempt.subject, attempt.object),
        "causes" => format!("Does {} cause {}?", attempt.subject, attempt.object),
        "has_property" => format!("Does a {} have {}?", attempt.subject, attempt.object),
        _ => format!("Does {} {} {}?", attempt.subject, attempt.relation, attempt.object),
    }
}

fn play_stop_reason_for_attempt(outcome: PlayAttemptOutcome) -> String {
    match outcome {
        PlayAttemptOutcome::SuccessCrystallized => "success".to_string(),
        PlayAttemptOutcome::FailurePersisted => "no_supporting_path".to_string(),
        PlayAttemptOutcome::SkippedKnownFailure => "anti_lobe_negative_match".to_string(),
        PlayAttemptOutcome::SkippedKnownSuccess => "success_already_known".to_string(),
    }
}

fn append_loop_audit_event(event: &LoopAuditRecord) {
    let payload = match serde_json::to_string(event) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("loop-audit serialization failed: {err}");
            return;
        }
    };

    if let Ok(path_value) = std::env::var("CSIF_LOOP_AUDIT_LOG_PATH") {
        let path = Path::new(&path_value);
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                eprintln!("loop-audit mkdir failed: {err}");
                return;
            }
        }
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(mut file) => {
                if let Err(err) = writeln!(file, "{payload}") {
                    eprintln!("loop-audit write failed: {err}");
                }
            }
            Err(err) => eprintln!("loop-audit open failed: {err}"),
        }
    } else {
        eprintln!("loop-audit {payload}");
    }
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(default)
}

fn parse_force_flag(value: Option<&str>) -> bool {
    matches!(
        value.map(|flag| flag.trim()),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}
