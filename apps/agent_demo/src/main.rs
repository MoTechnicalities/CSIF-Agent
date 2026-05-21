use csif_agent::{agent::CSIFAgent, server::start_server};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("=== CSIF Agent ===");
    let bank_path = std::env::var("CSIF_BANK_PATH").unwrap_or_else(|_| "./my_brain.rwif".to_string());
    let grammar_path = std::env::var("CSIF_GRAMMAR_PATH").unwrap_or_else(|_| "./grammar.toml".to_string());
    let port = std::env::var("CSIF_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8080);
    let agent = CSIFAgent::load_or_create_with_grammar(Path::new(&bank_path), Path::new(&grammar_path))
        .expect("Failed to load/create crystal bank");
    let agent_shared = Arc::new(Mutex::new(agent));

    if std::env::var("CSIF_LOBES_DIR").is_ok() {
        let poll_secs = std::env::var("CSIF_LOBES_POLL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(5);
        if poll_secs > 0 {
            let agent_for_poll = Arc::clone(&agent_shared);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(poll_secs));
                loop {
                    ticker.tick().await;
                    let mut guard = match agent_for_poll.lock() {
                        Ok(g) => g,
                        Err(_) => continue,
                    };
                    if let Ok(report) = guard.refresh_lobes_from_env() {
                        if report.applied > 0 {
                            eprintln!(
                                "Auto-loaded lobes: applied={} taught={} skipped={} ignored={}",
                                report.applied, report.taught, report.skipped, report.ignored
                            );
                        }
                    }
                }
            });
        }
    }

    start_server(agent_shared, port).await;
}
