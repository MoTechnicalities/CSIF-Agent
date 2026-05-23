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
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use crate::agent::{
    evaluate_instruction_execution, verify_proof_certificate, CSIFAgent,
    CompositeQuerySummary, ExplainResultPayload, InstructionExecutionDecision, ProofCertificate,
    QueryClauseResult, RequestTimeContext, RouteAuditTrail,
};
use crate::metadata::AppliedLobe;
use tokio_stream::iter;

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
        .route("/admin/execute-audit", get(admin_execute_audit_handler))
        .route("/v1/chat/completions", post(chat_completion_handler))
        .route("/v1/models", get(models_handler))
        .route("/v1/models/:model_id", get(model_handler))
        .with_state(agent)
}

pub async fn start_server(agent: Arc<Mutex<CSIFAgent>>, port: u16) {
    let app = build_app(agent);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    println!("CSIF Agent listening on port {}", port);
    axum::serve(listener, app).await.unwrap();
}

async fn query_handler(
    State(agent): State<Arc<Mutex<CSIFAgent>>>,
    Json(req): Json<QueryRequest>,
) -> Json<QueryResponse> {
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
    State(agent): State<Arc<Mutex<CSIFAgent>>>,
    Json(req): Json<QueryRequest>,
) -> Json<QueryResponse> {
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
    State(agent): State<Arc<Mutex<CSIFAgent>>>,
    Json(req): Json<ExplainRequest>,
) -> Json<ExplainResultPayload> {
    let agent = agent.lock().unwrap();
    Json(agent.explain_query(&req.text))
}

async fn visualize_handler(
    State(agent): State<Arc<Mutex<CSIFAgent>>>,
    Json(req): Json<VisualizeRequest>,
) -> Json<VisualizeResponse> {
    let format = req
        .format
        .as_deref()
        .unwrap_or("dot")
        .trim()
        .to_lowercase();

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
    State(agent): State<Arc<Mutex<CSIFAgent>>>,
) -> Result<Json<AdminLobesResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin_token(&headers)?;

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
    State(agent): State<Arc<Mutex<CSIFAgent>>>,
) -> Result<Json<AdminLobesReloadResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin_token(&headers)?;

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
    State(agent): State<Arc<Mutex<CSIFAgent>>>,
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
