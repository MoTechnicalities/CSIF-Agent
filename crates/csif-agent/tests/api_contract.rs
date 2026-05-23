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

struct EnvVarGuard {
    key: String,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(&self.key, previous);
        } else {
            std::env::remove_var(&self.key);
        }
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
    assert_eq!(query_time.get("initiator").and_then(Value::as_str), Some("user"));
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
    assert_eq!(explain_time.get("initiator").and_then(Value::as_str), Some("user"));
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
        Some("anti_lobe_negative_match")
    );
    assert!(
        explain_route
            .get("anti_lobe_bank_path")
            .and_then(Value::as_str)
            .map(|path| path.contains(".anti."))
            .unwrap_or(false)
    );
    assert!(
        explain_route
            .get("negative_evidence")
            .and_then(Value::as_array)
            .map(|items| {
                items.iter().any(|item| {
                    item.as_str()
                        .map(|line| line.contains("Explicit AntiLobe edge observed"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    );

    remove_if_exists(&bank_path);
    remove_if_exists(&grammar_path);
}

#[tokio::test]
async fn admin_anti_lobe_endpoint_exposes_persisted_negative_knowledge() {
    let bank_path = unique_temp_path("csif-agent-api-contract-anti-lobe", "rwif");
    let grammar_path = unique_temp_path("csif-agent-api-contract-anti-lobe", "grammar.rwif");

    let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path)
        .expect("failed to initialize test agent");
    let _ = agent.query_with_certificate("Is a whale a reptile?");

    let app = build_app(Arc::new(Mutex::new(agent)));

    let request = Request::builder()
        .method("GET")
        .uri("/admin/anti-lobe")
        .body(Body::empty())
        .expect("failed to build anti-lobe admin request");

    let response = app.oneshot(request).await.expect("anti-lobe admin request failed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read anti-lobe admin body");
    let json: Value = serde_json::from_slice(&body).expect("invalid anti-lobe admin json body");

    let anti_lobe = json.get("anti_lobe").expect("missing anti_lobe snapshot");
    assert!(
        anti_lobe
            .get("bank_path")
            .and_then(Value::as_str)
            .map(|path| path.contains(".anti."))
            .unwrap_or(false)
    );
    assert_eq!(anti_lobe.get("entry_count").and_then(Value::as_u64), Some(1));
    let entries = anti_lobe
        .get("entries")
        .and_then(Value::as_array)
        .expect("missing anti-lobe entries array");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.get("relation").and_then(Value::as_str), Some("not_is_a"));
    assert_eq!(entry.get("subject").and_then(Value::as_str), Some("whale"));
    assert_eq!(entry.get("object").and_then(Value::as_str), Some("reptile"));
    assert_eq!(entry.get("last_source_type").and_then(Value::as_str), Some("anti_lobe"));
    assert!(entry.get("last_phase").and_then(Value::as_f64).unwrap_or_default() > 3.0);

    remove_if_exists(&bank_path);
    remove_if_exists(&grammar_path);
    remove_if_exists(&bank_path.with_file_name(
        format!(
            "{}.anti.{}",
            bank_path.file_stem().and_then(|v| v.to_str()).unwrap_or("csif_agent_bank"),
            bank_path.extension().and_then(|v| v.to_str()).unwrap_or("json")
        )
    ));
}

#[tokio::test]
async fn admin_play_endpoint_exposes_scheduler_status_and_history_shape() {
    let bank_path = unique_temp_path("csif-agent-api-contract-play", "rwif");
    let grammar_path = unique_temp_path("csif-agent-api-contract-play", "grammar.rwif");

    let agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path)
        .expect("failed to initialize test agent");

    let app = build_app(Arc::new(Mutex::new(agent)));

    let request = Request::builder()
        .method("GET")
        .uri("/admin/play")
        .body(Body::empty())
        .expect("failed to build admin play request");

    let response = app.oneshot(request).await.expect("admin play request failed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read admin play response body");
    let json: Value = serde_json::from_slice(&body).expect("invalid admin play json body");

    let scheduler = json.get("scheduler").expect("missing scheduler section");
    assert!(scheduler.get("enabled").and_then(Value::as_bool).is_some());
    assert!(scheduler.get("poll_secs").and_then(Value::as_u64).unwrap_or_default() > 0);
    assert!(
        scheduler
            .get("max_cycles_per_tick")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            > 0
    );
    assert!(
        scheduler
            .get("history_limit")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            > 0
    );
    assert!(scheduler.get("stored_cycles").and_then(Value::as_u64).is_some());

    let recent_cycles = json
        .get("recent_cycles")
        .and_then(Value::as_array)
        .expect("missing recent_cycles array");
    assert!(recent_cycles.is_empty());

    remove_if_exists(&bank_path);
    remove_if_exists(&grammar_path);
}

#[tokio::test]
async fn admin_observation_endpoint_exposes_scheduler_status_and_history_shape() {
    let bank_path = unique_temp_path("csif-agent-api-contract-observation", "rwif");
    let grammar_path = unique_temp_path("csif-agent-api-contract-observation", "grammar.rwif");

    let agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path)
        .expect("failed to initialize test agent");

    let app = build_app(Arc::new(Mutex::new(agent)));

    let request = Request::builder()
        .method("GET")
        .uri("/admin/observation")
        .body(Body::empty())
        .expect("failed to build admin observation request");

    let response = app
        .oneshot(request)
        .await
        .expect("admin observation request failed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read admin observation response body");
    let json: Value =
        serde_json::from_slice(&body).expect("invalid admin observation json body");

    let scheduler = json.get("scheduler").expect("missing scheduler section");
    assert!(scheduler.get("enabled").and_then(Value::as_bool).is_some());
    assert!(scheduler.get("poll_secs").and_then(Value::as_u64).unwrap_or_default() > 0);
    assert!(scheduler.get("max_ms").and_then(Value::as_u64).unwrap_or_default() > 0);
    assert!(
        scheduler
            .get("history_limit")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            > 0
    );
    assert!(
        scheduler
            .get("source")
            .and_then(Value::as_str)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
    );
    assert!(
        scheduler
            .get("query")
            .and_then(Value::as_str)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
    );
    assert!(scheduler.get("stored_cycles").and_then(Value::as_u64).is_some());

    let recent_cycles = json
        .get("recent_cycles")
        .and_then(Value::as_array)
        .expect("missing recent_cycles array");
    assert!(recent_cycles.is_empty());

    remove_if_exists(&bank_path);
    remove_if_exists(&grammar_path);
}

#[tokio::test]
async fn admin_play_force_tick_returns_non_empty_cycle_with_system_play_initiator() {
    let _play_enabled = EnvVarGuard::set("CSIF_PLAY_ENABLED", "1");
    let _play_poll = EnvVarGuard::set("CSIF_PLAY_POLL_SECS", "60");
    let _play_max_cycles = EnvVarGuard::set("CSIF_PLAY_MAX_CYCLES_PER_TICK", "2");
    let _play_max_ms = EnvVarGuard::set("CSIF_PLAY_MAX_MS", "500");

    let bank_path = unique_temp_path("csif-agent-api-contract-play-force", "rwif");
    let grammar_path = unique_temp_path("csif-agent-api-contract-play-force", "grammar.rwif");

    let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path)
        .expect("failed to initialize test agent");
    assert_eq!(agent.teach("a whale is a mammal"), "[TEACHING] Knowledge crystallized.");
    assert_eq!(agent.teach("a mammal is an animal"), "[TEACHING] Knowledge crystallized.");
    assert_eq!(agent.teach("a mammal has vertebrate"), "[TEACHING] Knowledge crystallized.");

    let app = build_app(Arc::new(Mutex::new(agent)));

    let request = Request::builder()
        .method("GET")
        .uri("/admin/play?force=1")
        .body(Body::empty())
        .expect("failed to build admin play force request");

    let response = app
        .oneshot(request)
        .await
        .expect("admin play force request failed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read admin play force response body");
    let json: Value = serde_json::from_slice(&body).expect("invalid admin play force json body");

    assert_eq!(
        json.get("scheduler")
            .and_then(Value::as_object)
            .and_then(|scheduler| scheduler.get("enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let recent_cycles = json
        .get("recent_cycles")
        .and_then(Value::as_array)
        .expect("missing recent_cycles array");
    assert!(!recent_cycles.is_empty());

    let first_cycle = &recent_cycles[0];
    let audit_events = first_cycle
        .get("audit_events")
        .and_then(Value::as_array)
        .expect("missing play audit_events array");
    assert!(!audit_events.is_empty());
    assert!(audit_events.iter().all(|event| {
        event
            .get("initiator")
            .and_then(Value::as_str)
            .map(|value| value == "system:play")
            .unwrap_or(false)
    }));

    remove_if_exists(&bank_path);
    remove_if_exists(&grammar_path);
}

#[tokio::test]
async fn admin_observation_force_tick_returns_non_empty_cycle_with_system_observation_initiator() {
    let _observe_enabled = EnvVarGuard::set("CSIF_OBSERVE_ENABLED", "1");
    let _observe_poll = EnvVarGuard::set("CSIF_OBSERVE_POLL_SECS", "60");
    let _observe_max_ms = EnvVarGuard::set("CSIF_OBSERVE_MAX_MS", "500");
    let _observe_query = EnvVarGuard::set("CSIF_OBSERVE_QUERY", "Is a whale a reptile?");
    let _observe_source = EnvVarGuard::set("CSIF_OBSERVE_SOURCE", "test:observation");

    let bank_path = unique_temp_path("csif-agent-api-contract-observation-force", "rwif");
    let grammar_path = unique_temp_path("csif-agent-api-contract-observation-force", "grammar.rwif");

    let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path)
        .expect("failed to initialize test agent");
    assert_eq!(agent.teach("a whale is a mammal"), "[TEACHING] Knowledge crystallized.");

    let app = build_app(Arc::new(Mutex::new(agent)));

    let request = Request::builder()
        .method("GET")
        .uri("/admin/observation?force=1")
        .body(Body::empty())
        .expect("failed to build admin observation force request");

    let response = app
        .oneshot(request)
        .await
        .expect("admin observation force request failed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read admin observation force response body");
    let json: Value =
        serde_json::from_slice(&body).expect("invalid admin observation force json body");

    assert_eq!(
        json.get("scheduler")
            .and_then(Value::as_object)
            .and_then(|scheduler| scheduler.get("enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let recent_cycles = json
        .get("recent_cycles")
        .and_then(Value::as_array)
        .expect("missing recent_cycles array");
    assert!(!recent_cycles.is_empty());

    let first_cycle = &recent_cycles[0];
    let audit_events = first_cycle
        .get("audit_events")
        .and_then(Value::as_array)
        .expect("missing observation audit_events array");
    assert!(!audit_events.is_empty());
    assert!(audit_events.iter().all(|event| {
        event
            .get("initiator")
            .and_then(Value::as_str)
            .map(|value| value == "system:observation")
            .unwrap_or(false)
    }));

    remove_if_exists(&bank_path);
    remove_if_exists(&grammar_path);
}

#[tokio::test]
async fn admin_observation_force_tick_classifies_no_supporting_path_anomaly() {
    let _observe_enabled = EnvVarGuard::set("CSIF_OBSERVE_ENABLED", "1");
    let _observe_poll = EnvVarGuard::set("CSIF_OBSERVE_POLL_SECS", "60");
    let _observe_max_ms = EnvVarGuard::set("CSIF_OBSERVE_MAX_MS", "500");
    let _observe_query = EnvVarGuard::set("CSIF_OBSERVE_QUERY", "Is a whale a reptile?");
    let _observe_source = EnvVarGuard::set("CSIF_OBSERVE_SOURCE", "test:observation");
    let _observe_no_path_threshold = EnvVarGuard::set("CSIF_OBSERVE_NO_PATH_THRESHOLD", "1");
    let _observe_contradiction_threshold =
        EnvVarGuard::set("CSIF_OBSERVE_CONTRADICTION_THRESHOLD", "999");

    let bank_path = unique_temp_path("csif-agent-api-contract-observation-anomaly", "rwif");
    let grammar_path =
        unique_temp_path("csif-agent-api-contract-observation-anomaly", "grammar.rwif");

    let mut agent = CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path)
        .expect("failed to initialize test agent");
    assert_eq!(agent.teach("a whale is a mammal"), "[TEACHING] Knowledge crystallized.");

    let app = build_app(Arc::new(Mutex::new(agent)));

    let request = Request::builder()
        .method("GET")
        .uri("/admin/observation?force=1")
        .body(Body::empty())
        .expect("failed to build admin observation anomaly request");

    let response = app
        .oneshot(request)
        .await
        .expect("admin observation anomaly request failed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read admin observation anomaly response body");
    let json: Value =
        serde_json::from_slice(&body).expect("invalid admin observation anomaly json body");

    let recent_cycles = json
        .get("recent_cycles")
        .and_then(Value::as_array)
        .expect("missing recent_cycles array");
    assert!(!recent_cycles.is_empty());

    let first_cycle = &recent_cycles[0];
    let audit_events = first_cycle
        .get("audit_events")
        .and_then(Value::as_array)
        .expect("missing observation audit_events array");
    assert!(!audit_events.is_empty());

    let first_event = &audit_events[0];
    assert_eq!(
        first_event
            .get("stop_reason")
            .and_then(Value::as_str),
        Some("no_supporting_path")
    );
    assert_eq!(
        first_event
            .get("anomaly_classification")
            .and_then(Value::as_str),
        Some("no_supporting_path")
    );
    assert!(
        first_event
            .get("anomaly_score")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            >= 1
    );

    remove_if_exists(&bank_path);
    remove_if_exists(&grammar_path);
}
