use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use crate::agent::CSIFAgent;
use crate::metadata::AppliedLobe;
use tokio_stream::iter;

#[derive(Debug, Deserialize)]
struct QueryRequest {
    text: String,
}

#[derive(Debug, Serialize)]
struct QueryResponse {
    answer: String,
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
    content: String,
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

pub async fn start_server(agent: Arc<Mutex<CSIFAgent>>, port: u16) {
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/query", post(query_handler))
        .route("/teach", post(teach_handler))
        .route("/admin/lobes", get(admin_lobes_handler))
        .route("/admin/lobes/reload", post(admin_lobes_reload_handler))
        .route("/v1/chat/completions", post(chat_completion_handler))
        .route("/v1/models", get(models_handler))
        .route("/v1/models/:model_id", get(model_handler))
        .with_state(agent);

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
    let answer = agent.query(&req.text);
    Json(QueryResponse { answer })
}

async fn teach_handler(
    State(agent): State<Arc<Mutex<CSIFAgent>>>,
    Json(req): Json<QueryRequest>,
) -> Json<QueryResponse> {
    let mut agent = agent.lock().unwrap();
    let answer = agent.teach(&req.text);
    Json(QueryResponse { answer })
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
        .map(|message| message.content.clone())
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
                content: answer,
            },
            finish_reason: "stop".to_string(),
        }],
    };

    if stream {
        let payload = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
        let event_stream = iter(vec![
            Ok::<Event, Infallible>(Event::default().data(payload)),
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
