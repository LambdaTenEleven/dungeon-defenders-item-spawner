//! Read-only selected-export diagnostic using the adapter's streamed reader.
use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() < 2 {
        return Err("Usage: inspect <package> <export path> [--compare-memory]".into());
    }
    let start = Instant::now();
    let path = PathBuf::from(&arguments[0]);
    let selected =
        dd_catalog::package::Package::open_streamed(&path)?.inspect_export(&arguments[1])?;
    let elapsed = start.elapsed().as_secs_f64();
    if arguments
        .iter()
        .any(|argument| argument == "--compare-memory")
    {
        let memory = dd_catalog::package::Package::open(&path)?.inspect_export(&arguments[1])?;
        if selected != memory {
            return Err("Streamed and memory readers disagree".into());
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "path": selected.path, "class": selected.class_name, "archetype": selected.archetype,
            "is_archetype": selected.is_archetype, "is_class_default": selected.is_class_default,
            "properties": selected.properties.len(), "property_error": selected.property_error,
            "streamed_seconds": elapsed,
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}
