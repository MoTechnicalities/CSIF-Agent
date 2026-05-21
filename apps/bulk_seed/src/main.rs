use csif_agent::agent::CSIFAgent;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

fn usage() {
    eprintln!(
        "Usage: bulk_seed <bank_path> <grammar_path> <seed_dir>\n\n\
         Example:\n  bulk_seed /tmp/bank.rwif ./grammar.toml ./data/base_lobe_v1/seed"
    );
}

fn seed_file(agent: &mut CSIFAgent, file_path: &Path) -> (usize, usize) {
    let file = match File::open(file_path) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("Failed to open {}: {}", file_path.display(), err);
            return (0, 0);
        }
    };

    let mut taught = 0usize;
    let mut skipped = 0usize;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        match line {
            Ok(raw) => {
                let fact = raw.trim();
                if fact.is_empty() {
                    continue;
                }
                if agent.ingest_seed_fact(fact) {
                    taught += 1;
                } else {
                    skipped += 1;
                }
            }
            Err(_) => {
                skipped += 1;
            }
        }
    }

    (taught, skipped)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        usage();
        std::process::exit(2);
    }

    let bank_path = PathBuf::from(&args[1]);
    let grammar_path = PathBuf::from(&args[2]);
    let seed_dir = PathBuf::from(&args[3]);

    let mut agent = match CSIFAgent::load_or_create_with_grammar(&bank_path, &grammar_path) {
        Ok(a) => a,
        Err(err) => {
            eprintln!("Failed to initialize agent: {}", err);
            std::process::exit(1);
        }
    };

    let categories = [
        "taxonomy.txt",
        "causality.txt",
        "properties.txt",
        "geography.txt",
        "operator_utility.txt",
    ];

    let mut total_taught = 0usize;
    let mut total_skipped = 0usize;

    for category in categories {
        let path = seed_dir.join(category);
        println!("Bulk seeding: {}", category);
        let (taught, skipped) = seed_file(&mut agent, &path);
        total_taught += taught;
        total_skipped += skipped;
        println!("  taught={} skipped={}", taught, skipped);
    }

    if let Err(err) = agent.flush() {
        eprintln!("Failed to flush agent state: {}", err);
        std::process::exit(1);
    }

    println!("Bulk seed summary: taught={} skipped={}", total_taught, total_skipped);
}
