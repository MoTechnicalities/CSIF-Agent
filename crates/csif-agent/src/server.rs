use axum::{
    extract::State,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use crate::agent::CSIFAgent;

#[derive(Debug, Deserialize)]
struct QueryRequest {
    text: String,
}

#[derive(Debug, Serialize)]
struct QueryResponse {
    answer: String,
}

pub async fn start_server(agent: Arc<Mutex<CSIFAgent>>, port: u16) {
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/query", post(query_handler))
        .route("/teach", post(teach_handler))
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

async fn health_handler() -> &'static str {
    "ok"
}
