use eframe::egui;
use std::{fmt::Display, str::FromStr};

#[derive(Clone)]
struct Draft {
    text: String,
    last_value: String,
}

/// Keep incomplete text such as "-" visible without retaining a valid, stale request value.
/// The caller supplies an invalid value that its normal request validation rejects.
pub fn number_input<T: Display + FromStr>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut T,
    invalid: T,
) -> egui::Response {
    let id = ui.make_persistent_id(("number-input", label));
    let current = value.to_string();
    let mut draft = ui
        .data_mut(|data| data.get_temp::<Draft>(id))
        .unwrap_or_else(|| Draft {
            text: current.clone(),
            last_value: current.clone(),
        });
    if draft.last_value != current {
        draft.text = current;
    }
    let response = ui.add(
        egui::TextEdit::singleline(&mut draft.text)
            .id(id)
            .desired_width(90.0)
            .char_limit(16),
    );
    if response.changed() {
        *value = draft.text.trim().parse().unwrap_or(invalid);
    }
    draft.last_value = value.to_string();
    ui.data_mut(|data| data.insert_temp(id, draft));
    response
}
