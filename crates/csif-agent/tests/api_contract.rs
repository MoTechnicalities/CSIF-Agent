use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use csif_agent::{agent::CSIFAgent, server::build_app};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

fn unique_temp_path(prefix: &str, ext: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{ts}.{ext}"))
}

fn remove_if_exists(path: &Path) {
    if path.exists() {
        fs::remove_file(path).expect("failed to remove temporary file");
    }
}

#[tokio::test]
async fn query_and_explain_http_contract_exposes_route_and_time_audit_fields() {
    let bank_path = unique_temp_path("csif-agent-api-contract", "rwif");
    let grammar_path = unique_temp_path("csif-agent-api-contract", "grammar.rwif");

    let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path)
        .expect("failed to initialize test agent");
    assert_eq!(agent.teach("a whale is a mammal"), "[TEACHING] Knowledge crystallized.");
    assert_eq!(agent.teach("a mammal is an animal"), "[TEACHING] Knowledge crystallized.");

    let app = build_app(Arc::new(Mutex::new(agent)));

    let query_request = Request::builder()
        .method("POST")
        .uri("/query")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "text": "Is a whale an animal?" }).to_string()))
        .expect("failed to build query request");

    let query_response = app
        .clone()
        .oneshot(query_request)
        .await
        .expect("query request failed");
    assert_eq!(query_response.status(), StatusCode::OK);

    let query_body = to_bytes(query_response.into_body(), usize::MAX)
        .await
        .expect("failed to read query response body");
    let query_json: Value = serde_json::from_slice(&query_body).expect("invalid query json body");

    assert!(query_json.get("answer").and_then(Value::as_str).is_some());
    let query_time = query_json
        .get("request_time_context")
        .expect("missing query request_time_context");
    assert_eq!(query_time.get("timezone").and_then(Value::as_str), Some("UTC"));
    assert!(query_time.get("unix_ms").and_then(Value::as_i64).unwrap_or_default() > 0);
    let query_ts = query_time
        .get("request_received_at")
        .and_then(Value::as_str)
        .expect("missing query request_received_at");
    assert!(query_ts.ends_with('Z') || query_ts.ends_with("+00:00"));

    let query_route = query_json
        .get("route_audit")
        .expect("missing query route_audit");
    assert_eq!(query_route.get("relation").and_then(Value::as_str), Some("is_a"));
    assert_eq!(query_route.get("subject").and_then(Value::as_str), Some("whale"));
    assert_eq!(query_route.get("object").and_then(Value::as_str), Some("animal"));
    assert!(
        query_route
            .get("tried")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false)
    );
    assert!(query_route.get("stop_reason").and_then(Value::as_str).is_some());
    assert!(
        query_route
            .get("negative_evidence")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false)
    );

    let explain_request = Request::builder()
        .method("POST")
        .uri("/explain")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "text": "Is a whale an animal?" }).to_string()))
        .expect("failed to build explain request");

    let explain_response = app
        .oneshot(explain_request)
        .await
        .expect("explain request failed");
    assert_eq!(explain_response.status(), StatusCode::OK);

    let explain_body = to_bytes(explain_response.into_body(), usize::MAX)
        .await
        .expect("failed to read explain response body");
    let explain_json: Value =
        serde_json::from_slice(&explain_body).expect("invalid explain json body");

    assert_eq!(
        explain_json.get("intent").and_then(Value::as_str),
        Some("confirm_relation")
    );
    let explain_time = explain_json
        .get("request_time_context")
        .expect("missing explain request_time_context");
    assert_eq!(explain_time.get("timezone").and_then(Value::as_str), Some("UTC"));
    assert!(
        explain_time
            .get("unix_ms")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            > 0
    );

    let explain_route = explain_json
        .get("route_audit")
        .expect("missing explain route_audit");
    assert_eq!(explain_route.get("relation").and_then(Value::as_str), Some("is_a"));
    assert_eq!(explain_route.get("subject").and_then(Value::as_str), Some("whale"));
    assert_eq!(explain_route.get("object").and_then(Value::as_str), Some("animal"));
    assert!(
        explain_route
            .get("tried")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false)
    );
    assert!(explain_route.get("stop_reason").and_then(Value::as_str).is_some());

    remove_if_exists(&bank_path);
    remove_if_exists(&grammar_path);
}

#[tokio::test]
async fn query_and_explain_http_contract_exposes_no_path_route_stop_reason() {
    let bank_path = unique_temp_path("csif-agent-api-contract-no-path", "rwif");
    let grammar_path = unique_temp_path("csif-agent-api-contract-no-path", "grammar.rwif");

    let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path)
        .expect("failed to initialize test agent");
    assert_eq!(agent.teach("a whale is a mammal"), "[TEACHING] Knowledge crystallized.");

    let app = build_app(Arc::new(Mutex::new(agent)));

    let query_request = Request::builder()
        .method("POST")
        .uri("/query")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "text": "Is a whale a reptile?" }).to_string()))
        .expect("failed to build query request");

    let query_response = app
        .clone()
        .oneshot(query_request)
        .await
        .expect("query request failed");
    assert_eq!(query_response.status(), StatusCode::OK);

    let query_body = to_bytes(query_response.into_body(), usize::MAX)
        .await
        .expect("failed to read query response body");
    let query_json: Value = serde_json::from_slice(&query_body).expect("invalid query json body");

    assert!(
        query_json
            .get("answer")
            .and_then(Value::as_str)
            .map(|answer| answer.contains("NO:"))
            .unwrap_or(false)
    );
    assert!(query_json.get("request_time_context").is_some());
    let query_route = query_json
        .get("route_audit")
        .expect("missing query route_audit");
    assert_eq!(query_route.get("relation").and_then(Value::as_str), Some("is_a"));
    assert_eq!(query_route.get("subject").and_then(Value::as_str), Some("whale"));
    assert_eq!(query_route.get("object").and_then(Value::as_str), Some("reptile"));
    assert_eq!(
        query_route.get("stop_reason").and_then(Value::as_str),
        Some("no_supporting_path")
    );
    assert!(
        query_route
            .get("negative_evidence")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false)
    );

    let explain_request = Request::builder()
        .method("POST")
        .uri("/explain")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "text": "Is a whale a reptile?" }).to_string()))
        .expect("failed to build explain request");

    let explain_response = app
        .oneshot(explain_request)
        .await
        .expect("explain request failed");
    assert_eq!(explain_response.status(), StatusCode::OK);

    let explain_body = to_bytes(explain_response.into_body(), usize::MAX)
        .await
        .expect("failed to read explain response body");
    let explain_json: Value =
        serde_json::from_slice(&explain_body).expect("invalid explain json body");

    assert!(explain_json.get("request_time_context").is_some());
    let explain_route = explain_json
        .get("route_audit")
        .expect("missing explain route_audit");
    assert_eq!(
        explain_route.get("stop_reason").and_then(Value::as_str),
        Some("no_supporting_path")
    );

    remove_if_exists(&bank_path);
    remove_if_exists(&grammar_path);
}
