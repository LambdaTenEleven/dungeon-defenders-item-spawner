//! Live local-game benchmark. Uses the production search/transport and may spawn matching pickups.
//! Keep this executable beside the packaged adapter host and DLL. See docs/reroll-benchmark.md.
// Shared application modules include entry points unused by this diagnostic.
#[allow(dead_code)]
#[path = "../src/connection.rs"]
mod connection;
#[allow(dead_code)]
#[path = "../src/game.rs"]
mod game;
#[allow(dead_code)]
#[path = "../src/search.rs"]
mod search;
#[allow(dead_code)]
#[path = "../src/settings.rs"]
mod settings;

use dd_protocol::{Command, ItemOptions, LootConditions, Quality, SpawnOptions};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_NULL,
};

fn probe(window: usize, stop: &AtomicBool) -> Vec<(f64, bool)> {
    let mut samples = Vec::new();
    while !stop.load(Ordering::Acquire) {
        let started = Instant::now();
        // SAFETY: the HWND was checked against the attached PID. WM_NULL has no pointer arguments
        // or game-state effects. The bounded call needs no result pointer.
        let ok = unsafe {
            SendMessageTimeoutW(
                window as _,
                WM_NULL,
                0,
                0,
                SMTO_ABORTIFHUNG,
                250,
                std::ptr::null_mut(),
            )
        } != 0;
        samples.push((started.elapsed().as_secs_f64() * 1000.0, ok));
        std::thread::sleep(Duration::from_millis(20));
    }
    samples
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(5..=6).contains(&args.len()) {
        return Err("Usage: benchmark_search <catalog.json> <game-window-handle> <template> <attempts> <comma-separated-delays-ms> [seconds=120]. Local game only; matching items may spawn.".into());
    }
    let snapshot: dd_catalog::CatalogSnapshot =
        serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let window = args[1].parse::<usize>().map_err(|e| e.to_string())?;
    let attempts = args[3].parse::<u32>().map_err(|e| e.to_string())?;
    let limits = settings::SearchLimits {
        attempts,
        seconds: args
            .get(5)
            .map_or(Ok(120), |value| value.parse::<u32>())
            .map_err(|e| e.to_string())?,
    };
    limits.validate().map_err(str::to_owned)?;
    let pauses = args[4]
        .split(',')
        .map(|value| value.parse::<u64>().map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if pauses.is_empty() || pauses.iter().any(|pause| !(1..=50).contains(pause)) {
        return Err("Benchmark pauses must be between 1 and 50 ms.".into());
    }
    let (connection, ready) = connection::GameConnection::start(snapshot.key.clone())?;
    if !ready.can_give {
        return Err(ready.message);
    }
    let mut window_pid = 0;
    // SAFETY: query only, and window_pid is a writable local u32.
    unsafe {
        GetWindowThreadProcessId(window as _, &mut window_pid);
    }
    if window_pid != connection.pid {
        return Err("Window does not belong to the attached game.".into());
    }
    let inspected = connection::request(
        connection.pid,
        Command::InspectItem {
            catalog: snapshot.key.clone(),
            template: args[2].clone(),
        },
    )?;
    if !inspected
        .inspection
        .as_ref()
        .is_some_and(|item| item.available)
    {
        return Err(format!("Template unavailable: {}", inspected.message));
    }
    for pause in pauses {
        let stop = Arc::new(AtomicBool::new(false));
        let probe_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || probe(window, &probe_stop));
        let mut rolls = 0;
        let started = Instant::now();
        let result = search::run_paced(
            connection.pid,
            Command::Spawn {
                catalog: snapshot.key.clone(),
                template: args[2].clone(),
                quantity: 1,
                options: SpawnOptions::Equipment {
                    options: ItemOptions::RandomRoll {
                        strength: 1.0,
                        quality: Some(Quality::UltimatePlusPlus),
                        conditions: LootConditions::CurrentMission,
                    },
                },
            },
            limits,
            &AtomicBool::new(false),
            |_| rolls += 1,
            Duration::from_millis(pause),
        );
        let seconds = started.elapsed().as_secs_f64();
        stop.store(true, Ordering::Release);
        let samples = worker.join().map_err(|_| "Probe thread panicked")?;
        let timeouts = samples.iter().filter(|(_, ok)| !ok).count();
        let mut latency: Vec<_> = samples.iter().map(|(ms, _)| *ms).collect();
        latency.sort_by(f64::total_cmp);
        println!(
            "{}",
            serde_json::json!({
                "template": args[2], "pause_ms": pause, "attempts": rolls,
                "seconds": seconds, "rolls_per_second": f64::from(rolls) / seconds,
                "probe_samples": latency.len(), "probe_failures": timeouts,
                "probe_p95_ms": latency.get(latency.len().saturating_sub(1) * 95 / 100),
                "probe_max_ms": latency.last(), "result": result,
            })
        );
        let outcome = result?;
        if timeouts > 0 {
            return Err("Game responsiveness probe failed; benchmark stopped.".into());
        }
        if outcome.spawn.is_some_and(|spawn| spawn.spawned > 0) {
            return Err(
                "A matching pickup spawned. Benchmark stopped; collect it normally.".into(),
            );
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}
