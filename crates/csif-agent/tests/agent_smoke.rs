use csif_agent::agent::CSIFAgent;
use std::path::Path;

#[test]
fn agent_load_or_create_smoke() {
    let path = Path::new("/tmp/csif-agent-test.rwif");
    if path.exists() {
        std::fs::remove_file(path).unwrap();
    }
    let agent = CSIFAgent::load_or_create(path);
    assert!(agent.is_ok());
    let mut agent = agent.unwrap();
    let res = agent.query("test query");
    assert!(!res.is_empty());
}
