//! Pace single-candidate requests outside the game process. Never retry ambiguous failures.
use crate::settings::SearchLimits;
use dd_protocol::{AdapterStatus, Command, SpawnKind, SpawnOutcome};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

// Live 50/25/10 ms comparison: docs/reroll-benchmark.md. Keep one request in flight.
const PAUSE: Duration = Duration::from_millis(10);

pub fn run(
    pid: u32,
    command: Command,
    limits: SearchLimits,
    cancel: &AtomicBool,
    progress: impl FnMut(String),
) -> Result<AdapterStatus, String> {
    run_paced(pid, command, limits, cancel, progress, PAUSE)
}

/// Sequential pacing override for the live benchmark; engine calls remain serialized.
pub fn run_paced(
    pid: u32,
    command: Command,
    limits: SearchLimits,
    cancel: &AtomicBool,
    progress: impl FnMut(String),
    pause: Duration,
) -> Result<AdapterStatus, String> {
    let started = Instant::now();
    run_with(
        pid,
        command,
        limits,
        cancel,
        progress,
        |command| crate::connection::request(pid, command),
        (|| std::thread::sleep(pause), || started.elapsed()),
    )
}

fn run_with(
    pid: u32,
    command: Command,
    limits: SearchLimits,
    cancel: &AtomicBool,
    mut progress: impl FnMut(String),
    mut request: impl FnMut(Command) -> Result<AdapterStatus, String>,
    (mut pause, elapsed): (impl FnMut(), impl Fn() -> Duration),
) -> Result<AdapterStatus, String> {
    let Command::Spawn {
        catalog,
        template,
        quantity,
        options,
    } = command
    else {
        return Err("Expected an item request.".into());
    };
    dd_protocol::validate_grant(&template, quantity).map_err(str::to_string)?;
    options.validate().map_err(str::to_string)?;
    if !options.needs_search() {
        return request(Command::Spawn {
            catalog,
            template,
            quantity,
            options,
        });
    }
    limits.validate().map_err(str::to_owned)?;
    let max_time = Duration::from_secs(u64::from(limits.seconds));
    let mut attempts = 0;
    let mut spawned = 0;
    while spawned < quantity
        && attempts < limits.attempts
        && elapsed() < max_time
        && !cancel.load(Ordering::Acquire)
    {
        if attempts > 0 {
            pause();
            if cancel.load(Ordering::Acquire) || elapsed() >= max_time {
                break;
            }
        }
        let status = request(Command::Spawn {
            catalog: catalog.clone(),
            template: template.clone(),
            quantity: 1,
            options: options.clone(),
        })
        .map_err(|error| {
            format!("{spawned}/{quantity} pickups confirmed before search stopped. {error}")
        })?;
        if status.catalog.as_ref() != Some(&catalog) {
            return Err(format!(
                "{spawned}/{quantity} pickups confirmed. Catalog generation changed; search stopped without retrying."
            ));
        }
        attempts += 1;
        match (status.roll_matched, status.spawn.as_ref()) {
            (
                Some(true),
                Some(SpawnOutcome {
                    requested: 1,
                    spawned: 1,
                    kind: SpawnKind::EquipmentPickup,
                    uncertain: false,
                }),
            ) => spawned += 1,
            (
                Some(false),
                Some(SpawnOutcome {
                    requested: 1,
                    spawned: 0,
                    kind: SpawnKind::EquipmentPickup,
                    uncertain: false,
                }),
            ) => {}
            _ => {
                return Err(format!(
                    "{spawned}/{quantity} pickups confirmed. Missing or inconsistent spawn result; search stopped. Check nearby pickups before retrying."
                ));
            }
        }
        if !status.can_give {
            return Err(format!(
                "{spawned}/{quantity} pickups confirmed. Game is no longer ready."
            ));
        }
        progress(format!(
            "Searching: {attempts}/{} rolls, {:.1}/{} seconds, {spawned}/{quantity} pickups spawned.",
            limits.attempts,
            elapsed().as_secs_f64(),
            limits.seconds
        ));
    }
    let reason = if spawned == quantity {
        "Finished"
    } else if cancel.load(Ordering::Acquire) {
        "Cancelled"
    } else if attempts >= limits.attempts {
        "Attempt limit reached"
    } else {
        "Time limit reached"
    };
    Ok(AdapterStatus {
        pid,
        catalog: Some(catalog),
        bits: 32,
        diagnostic: false,
        can_give: true,
        roll_matched: None,
        inspection: None,
        spawn: Some(SpawnOutcome {
            requested: quantity,
            spawned,
            kind: SpawnKind::EquipmentPickup,
            uncertain: false,
        }),
        message: format!(
            "{reason}: {spawned}/{quantity} pickups spawned after {attempts} rolls ({:.1} seconds). Unmatched rolls were discarded. Pick up matching items near your hero.",
            elapsed().as_secs_f64()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dd_protocol::{ItemOptions, SpawnOptions};
    fn key() -> dd_catalog::CatalogKey {
        dd_catalog::CatalogKey {
            installation_id: "a".repeat(64),
            generation: "b".repeat(64),
        }
    }
    fn command(quantity: u8) -> Command {
        Command::Spawn {
            catalog: key(),
            template: "Package.Item".into(),
            quantity,
            options: SpawnOptions::Equipment {
                options: ItemOptions::RandomRoll {
                    strength: 1.0,
                    quality: Some(dd_protocol::Quality::Mythical),
                    conditions: dd_protocol::LootConditions::CurrentMission,
                },
            },
        }
    }
    fn status(matched: bool) -> AdapterStatus {
        AdapterStatus {
            pid: 1,
            catalog: Some(key()),
            bits: 32,
            diagnostic: false,
            can_give: true,
            roll_matched: Some(matched),
            inspection: None,
            spawn: Some(SpawnOutcome {
                requested: 1,
                spawned: u8::from(matched),
                kind: SpawnKind::EquipmentPickup,
                uncertain: false,
            }),
            message: String::new(),
        }
    }
    #[test]
    fn misses_are_paced_and_only_matching_rolls_count() {
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let mut pauses = 0;
        let result = run_with(
            1,
            command(2),
            SearchLimits::default(),
            &cancel,
            |_| {},
            |cmd| {
                assert!(matches!(cmd, Command::Spawn { quantity: 1, .. }));
                calls += 1;
                Ok(status(calls % 2 == 0))
            },
            (|| pauses += 1, || Duration::ZERO),
        )
        .unwrap();
        assert_eq!((calls, pauses), (4, 3));
        assert!(result.message.starts_with("Finished: 2/2"));
    }
    #[test]
    fn impossible_roll_stops_at_attempt_limit() {
        let mut calls = 0;
        let result = run_with(
            1,
            command(1),
            SearchLimits::default(),
            &AtomicBool::new(false),
            |_| {},
            |_| {
                calls += 1;
                Ok(status(false))
            },
            (|| {}, || Duration::ZERO),
        )
        .unwrap();
        assert_eq!(calls, SearchLimits::default().attempts);
        assert!(result.message.starts_with("Attempt limit reached: 0/1"));
    }
    #[test]
    fn cancellation_keeps_confirmed_pickups_and_schedules_no_more() {
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = run_with(
            1,
            command(2),
            SearchLimits::default(),
            &cancel,
            |_| {},
            |_| {
                calls += 1;
                cancel.store(true, Ordering::Release);
                Ok(status(true))
            },
            (|| {}, || Duration::ZERO),
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert!(result.message.starts_with("Cancelled: 1/2"));
    }
    #[test]
    fn changed_catalog_stops_without_retrying() {
        let mut calls = 0;
        let result = run_with(
            1,
            command(2),
            SearchLimits::default(),
            &AtomicBool::new(false),
            |_| {},
            |_| {
                calls += 1;
                let mut response = status(true);
                response.catalog.as_mut().unwrap().generation = "c".repeat(64);
                Ok(response)
            },
            (|| {}, || Duration::ZERO),
        );
        assert!(result.unwrap_err().contains("Catalog generation changed"));
        assert_eq!(calls, 1);
    }

    #[test]
    fn communication_error_is_never_retried() {
        let mut calls = 0;
        let result = run_with(
            1,
            command(1),
            SearchLimits::default(),
            &AtomicBool::new(false),
            |_| {},
            |_| {
                calls += 1;
                Err("Unconfirmed completion".into())
            },
            (|| {}, || Duration::ZERO),
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn unconfirmed_or_inconsistent_success_is_never_retried() {
        for outcome in [
            None,
            Some(SpawnOutcome {
                requested: 1,
                spawned: 0,
                kind: SpawnKind::EquipmentPickup,
                uncertain: false,
            }),
            Some(SpawnOutcome {
                requested: 1,
                spawned: 1,
                kind: SpawnKind::EquipmentPickup,
                uncertain: true,
            }),
        ] {
            let mut calls = 0;
            let result = run_with(
                1,
                command(2),
                SearchLimits::default(),
                &AtomicBool::new(false),
                |_| {},
                |_| {
                    calls += 1;
                    let mut response = status(true);
                    response.spawn = outcome;
                    Ok(response)
                },
                (|| {}, || Duration::ZERO),
            );
            assert!(result.is_err());
            assert_eq!(calls, 1);
        }
    }

    #[test]
    fn communication_failure_preserves_partial_confirmed_count() {
        let mut calls = 0;
        let result = run_with(
            1,
            command(2),
            SearchLimits::default(),
            &AtomicBool::new(false),
            |_| {},
            |_| {
                calls += 1;
                if calls == 1 {
                    Ok(status(true))
                } else {
                    Err("Ambiguous completion".into())
                }
            },
            (|| {}, || Duration::ZERO),
        );
        assert_eq!(calls, 2);
        assert!(result.unwrap_err().starts_with("1/2 pickups confirmed"));
    }

    #[test]
    fn configured_attempt_limit_counts_the_entire_batch() {
        for limit in [1, 3, 700] {
            let mut calls = 0;
            let result = run_with(
                1,
                command(2),
                SearchLimits {
                    attempts: limit,
                    seconds: 15,
                },
                &AtomicBool::new(false),
                |_| {},
                |_| {
                    calls += 1;
                    Ok(status(calls == 1))
                },
                (|| {}, || Duration::ZERO),
            )
            .unwrap();
            assert_eq!(calls, limit);
            assert_eq!(result.spawn.unwrap().spawned, 1);
            assert!(result.message.starts_with("Attempt limit reached: 1/2"));
        }
    }

    #[test]
    fn deadline_during_request_preserves_result_and_starts_no_more_work() {
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let mut calls = 0;
        let result = run_with(
            1,
            command(2),
            SearchLimits {
                attempts: 700,
                seconds: 2,
            },
            &AtomicBool::new(false),
            |_| {},
            |_| {
                calls += 1;
                elapsed.set(Duration::from_secs(3));
                Ok(status(true))
            },
            (|| {}, || elapsed.get()),
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(result.spawn.unwrap().spawned, 1);
        assert!(result.message.starts_with("Time limit reached: 1/2"));
    }

    #[test]
    fn deadline_or_cancel_during_pause_prevents_the_next_request() {
        for cancelled in [false, true] {
            let elapsed = std::cell::Cell::new(Duration::ZERO);
            let cancel = AtomicBool::new(false);
            let mut calls = 0;
            let result = run_with(
                1,
                command(2),
                SearchLimits {
                    attempts: 700,
                    seconds: 2,
                },
                &cancel,
                |_| {},
                |_| {
                    calls += 1;
                    Ok(status(false))
                },
                (
                    || {
                        if cancelled {
                            cancel.store(true, Ordering::Release);
                        } else {
                            elapsed.set(Duration::from_secs(2));
                        }
                    },
                    || elapsed.get(),
                ),
            )
            .unwrap();
            assert_eq!(calls, 1);
            assert!(result.message.starts_with(if cancelled {
                "Cancelled"
            } else {
                "Time limit reached"
            }));
        }
    }

    #[test]
    fn invalid_limits_never_dispatch_a_search() {
        let result = run_with(
            1,
            command(1),
            SearchLimits {
                attempts: 0,
                seconds: 15,
            },
            &AtomicBool::new(false),
            |_| {},
            |_| panic!("invalid search was dispatched"),
            (|| {}, || Duration::ZERO),
        );
        assert!(result.unwrap_err().contains("Maximum roll attempts"));
    }

    #[test]
    fn ordinary_spawns_are_not_blocked_by_unused_search_settings() {
        let mut request = command(2);
        if let Command::Spawn { options, .. } = &mut request {
            *options = SpawnOptions::Equipment {
                options: ItemOptions::TemplateDefaults,
            };
        }
        let mut calls = 0;
        run_with(
            1,
            request,
            SearchLimits {
                attempts: 0,
                seconds: 0,
            },
            &AtomicBool::new(false),
            |_| panic!("ordinary spawn reported search progress"),
            |command| {
                assert!(matches!(command, Command::Spawn { quantity: 2, .. }));
                calls += 1;
                Ok(status(true))
            },
            (|| panic!("ordinary spawn was paced"), || Duration::ZERO),
        )
        .unwrap();
        assert_eq!(calls, 1);
    }
}
