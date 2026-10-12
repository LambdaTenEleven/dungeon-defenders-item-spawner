//! A selection generation prevents late inspection responses from authorizing another item.
use dd_protocol::ItemInspection;
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct InspectionState {
    path: String,
    generation: u64,
    changed: Instant,
    pending: Option<u64>,
    requested: bool,
    pub result: Option<ItemInspection>,
    error: Option<String>,
}

impl Default for InspectionState {
    fn default() -> Self {
        Self {
            path: String::new(),
            generation: 0,
            changed: Instant::now(),
            pending: None,
            requested: false,
            result: None,
            error: None,
        }
    }
}

impl InspectionState {
    pub fn select(&mut self, path: &str) {
        if self.path != path {
            self.path = path.to_owned();
            self.invalidate();
        }
    }

    /// Also called when reconnecting or detaching, even if the path stays the same.
    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.changed = Instant::now();
        self.requested = false;
        self.result = None;
        self.error = None;
    }

    pub fn begin(&mut self, now: Instant) -> Option<(u64, String)> {
        if self.pending.is_some() || self.requested || now.duration_since(self.changed) < DEBOUNCE {
            return None;
        }
        self.requested = true;
        self.pending = Some(self.generation);
        Some((self.generation, self.path.clone()))
    }

    pub fn complete(&mut self, generation: u64, result: Result<ItemInspection, String>) -> bool {
        if self.pending == Some(generation) {
            self.pending = None;
        }
        if self.generation != generation {
            return false;
        }
        match result {
            Ok(result) if result.template == self.path => {
                self.result = Some(result);
            }
            Ok(_) => {
                self.error =
                    Some("Inspection returned a different item. Recheck availability.".into());
            }
            Err(error) => {
                self.error = Some(error);
            }
        }
        true
    }

    pub fn available(&self, path: &str) -> bool {
        self.path == path
            && self.error.is_none()
            && self
                .result
                .as_ref()
                .is_some_and(|result| result.template == path && result.available)
    }

    pub fn reason(&self) -> &str {
        if let Some(error) = &self.error {
            return error;
        }
        match &self.result {
            Some(result) if result.available => result
                .reason
                .as_deref()
                .unwrap_or("Ready to spawn in the current game context."),
            Some(result) => result
                .reason
                .as_deref()
                .unwrap_or("This item is unavailable in the current game context."),
            None if self.pending.is_some() => "Checking selected item availabilityвЂ¦",
            None => "Selected item availability has not been checked.",
        }
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(path: &str) -> ItemInspection {
        ItemInspection {
            template: path.into(),
            available: true,
            reason: None,
            capabilities: dd_protocol::ItemCapabilities {
                template_defaults: true,
                ..Default::default()
            },
        }
    }

    #[test]
    fn only_latest_selection_can_be_authorized() {
        let mut state = InspectionState::default();
        state.select("Package.First");
        let (old, _) = state.begin(Instant::now() + DEBOUNCE).unwrap();
        state.select("Package.Second");
        assert!(state.begin(Instant::now() + DEBOUNCE).is_none());
        assert!(!state.complete(old, Ok(ready("Package.First"))));
        assert!(!state.available("Package.First") && !state.available("Package.Second"));
        let (new, _) = state.begin(Instant::now() + DEBOUNCE).unwrap();
        assert!(state.complete(new, Ok(ready("Package.Second"))));
        assert!(state.available("Package.Second"));
        assert!(!state.available("Package.First"));
    }

    #[test]
    fn reconnect_invalidates_even_identical_item_response() {
        let mut state = InspectionState::default();
        state.select("Package.Item");
        let (old, _) = state.begin(Instant::now() + DEBOUNCE).unwrap();
        state.invalidate();
        assert!(!state.complete(old, Ok(ready("Package.Item"))));
        assert!(!state.available("Package.Item"));
    }

    #[test]
    fn typing_is_debounced_and_failures_are_not_retried_every_frame() {
        let mut state = InspectionState::default();
        state.select("Package.Item");
        assert!(state.begin(Instant::now()).is_none());
        let (generation, _) = state.begin(Instant::now() + DEBOUNCE).unwrap();
        state.complete(generation, Err("Package is not loaded.".into()));
        assert!(state.begin(Instant::now() + DEBOUNCE).is_none());
        assert!(!state.available("Package.Item"));
        assert_eq!(state.reason(), "Package is not loaded.");
        state.invalidate();
        assert!(state.begin(Instant::now() + DEBOUNCE).is_some());
    }
}
