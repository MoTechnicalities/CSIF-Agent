use csif_agent::{agent::CSIFAgent, server::start_server};
use std::sync::{Arc, Mutex};
use std::path::Path;

#[tokio::main]
async fn main() {
    println!("=== CSIF Agent ===");
    let bank_path = std::env::var("CSIF_BANK_PATH").unwrap_or_else(|_| "./my_brain.rwif".to_string());
    let agent = CSIFAgent::load_or_create(Path::new(&bank_path))
        .expect("Failed to load/create crystal bank");
    let agent_shared = Arc::new(Mutex::new(agent));
    start_server(agent_shared, 8080).await;
}
