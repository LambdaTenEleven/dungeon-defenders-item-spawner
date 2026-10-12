//! Developer diagnostic. Writes metadata only; package files are always read-only.
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Instant};

fn main() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() < 2 {
        return Err("Usage: scan <installation> <cache directory> [--force]".into());
    }
    let start = Instant::now();
    let snapshot = dd_catalog::scan_at(
        &PathBuf::from(&arguments[0]),
        &PathBuf::from(&arguments[1]),
        arguments.iter().any(|argument| argument == "--force"),
        &AtomicBool::new(false),
        |progress| {
            if progress.completed % 10 == 0 || progress.completed == progress.total {
                eprintln!(
                    "{}/{} {}",
                    progress.completed, progress.total, progress.message
                );
            }
        },
    )?;
    let loaded = dd_catalog::cache::load_cached_at(&PathBuf::from(&arguments[1]), &snapshot.key)?;
    if loaded.entries != snapshot.entries {
        return Err("Reloaded catalog differs from the generated snapshot".into());
    }
    let mut counts = std::collections::BTreeMap::new();
    for entry in &snapshot.entries {
        *counts.entry(entry.kind.label()).or_insert(0) += 1;
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"seconds":start.elapsed().as_secs_f64(),"key":snapshot.key,"packages":snapshot.packages.len(),"entries":snapshot.entries.len(),"counts":counts,"unavailable":snapshot.entries.iter().filter(|entry| entry.unavailable_reason.is_some()).count(),"issues":snapshot.issues})).map_err(|error| error.to_string())?);
    Ok(())
}
