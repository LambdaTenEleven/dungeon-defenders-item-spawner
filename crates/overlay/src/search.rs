//! Pace single-candidate requests outside the game process. Never retry ambiguous failures.
use dd_protocol::{AdapterStatus, Command};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_ATTEMPTS: usize = 200;
const MAX_TIME: Duration = Duration::from_secs(15);
const PAUSE: Duration = Duration::from_millis(50);

pub fn run(
    pid: u32,
    command: Command,
    cancel: &AtomicBool,
    progress: impl FnMut(String),
) -> Result<AdapterStatus, String> {
    run_with(
        pid,
        command,
        cancel,
        progress,
        |command| crate::connection::request(pid, command),
        || std::thread::sleep(PAUSE),
    )
}

fn run_with(
    pid: u32,
    command: Command,
    cancel: &AtomicBool,
    mut progress: impl FnMut(String),
    mut request: impl FnMut(Command) -> Result<AdapterStatus, String>,
    mut pause: impl FnMut(),
) -> Result<AdapterStatus, String> {
    let Command::Give {
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
        return request(Command::Give {
            template,
            quantity,
            options,
        });
    }
    let started = Instant::now();
    let mut attempts = 0;
    let mut spawned = 0;
    while spawned < quantity
        && attempts < MAX_ATTEMPTS
        && started.elapsed() < MAX_TIME
        && !cancel.load(Ordering::Acquire)
    {
        if attempts > 0 {
            pause();
            if cancel.load(Ordering::Acquire) || started.elapsed() >= MAX_TIME {
                break;
            }
        }
        let status = request(Command::Give {
            template: template.clone(),
            quantity: 1,
            options: options.clone(),
        })
        .map_err(|error| {
            format!("{spawned}/{quantity} pickups confirmed before search stopped. {error}")
        })?;
        attempts += 1;
        match status.roll_matched {
            Some(true) => spawned += 1,
            Some(false) => {}
            None => {
                return Err(format!(
                    "{spawned}/{quantity} pickups confirmed. Missing roll result; search stopped. Check nearby pickups before retrying."
                ));
            }
        }
        if !status.can_give {
            return Err(format!(
                "{spawned}/{quantity} pickups confirmed. Game is no longer ready."
            ));
        }
        progress(format!(
            "Searching: {attempts}/{MAX_ATTEMPTS} rolls, {spawned}/{quantity} pickups spawned."
        ));
    }
    let reason = if spawned == quantity {
        "Finished"
    } else if cancel.load(Ordering::Acquire) {
        "Cancelled"
    } else {
        "Search limit reached"
    };
    Ok(AdapterStatus {
        pid,
        bits: 32,
        diagnostic: false,
        can_give: true,
        roll_matched: None,
        message: format!(
            "{reason}: {spawned}/{quantity} pickups spawned after {attempts} rolls. Unmatched rolls were discarded. Pick up matching items near your hero."
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dd_protocol::ItemOptions;
    fn command(quantity: u8) -> Command {
        Command::Give {
            template: "Package.Item".into(),
            quantity,
            options: ItemOptions::RandomRoll {
                strength: 1.0,
                quality: Some(dd_protocol::Quality::Mythical),
                conditions: dd_protocol::LootConditions::CurrentMission,
            },
        }
    }
    fn status(matched: bool) -> AdapterStatus {
        AdapterStatus {
            pid: 1,
            bits: 32,
            diagnostic: false,
            can_give: true,
            roll_matched: Some(matched),
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
            &cancel,
            |_| {},
            |cmd| {
                assert!(matches!(cmd, Command::Give { quantity: 1, .. }));
                calls += 1;
                Ok(status(calls % 2 == 0))
            },
            || pauses += 1,
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
            &AtomicBool::new(false),
            |_| {},
            |_| {
                calls += 1;
                Ok(status(false))
            },
            || {},
        )
        .unwrap();
        assert_eq!(calls, MAX_ATTEMPTS);
        assert!(result.message.starts_with("Search limit reached: 0/1"));
    }
    #[test]
    fn cancellation_keeps_confirmed_pickups_and_schedules_no_more() {
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = run_with(
            1,
            command(2),
            &cancel,
            |_| {},
            |_| {
                calls += 1;
                cancel.store(true, Ordering::Release);
                Ok(status(true))
            },
            || {},
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert!(result.message.starts_with("Cancelled: 1/2"));
    }
    #[test]
    fn communication_error_is_never_retried() {
        let mut calls = 0;
        let result = run_with(
            1,
            command(1),
            &AtomicBool::new(false),
            |_| {},
            |_| {
                calls += 1;
                Err("Unconfirmed completion".into())
            },
            || {},
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}
