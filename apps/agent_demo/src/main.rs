use csif_agent::{agent::CSIFAgent, server::start_server};
use std::sync::{Arc, Mutex};
use std::path::Path;

#[tokio::main]
async fn main() {
    println!("=== CSIF Agent ===");
    let agent = CSIFAgent::load_or_create(Path::new("./my_brain.rwif"))
        .expect("Failed to load/create crystal bank");
    let agent_shared = Arc::new(Mutex::new(agent));
    start_server(agent_shared, 8080).await;
}
