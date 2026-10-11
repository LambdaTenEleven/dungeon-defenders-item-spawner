use crate::number_input::number_input;
use dd_protocol::{
    CustomItem, ItemOptions, Quality,
    items::{DAMAGE_LIMIT, HERO_STATS, STAT_LIMIT, UPGRADE_LIMIT},
};
use eframe::egui;

#[derive(Default, PartialEq)]
enum Mode {
    #[default]
    Template,
    Random,
    Custom,
}

pub struct ItemEditor {
    mode: Mode,
    strength: f32,
    quality: Option<Quality>,
    conditions: dd_protocol::LootConditions,
    custom: CustomItem,
}

impl Default for ItemEditor {
    fn default() -> Self {
        Self {
            mode: Mode::Template,
            strength: 1.0,
            quality: None,
            conditions: dd_protocol::LootConditions::CurrentMission,
            custom: CustomItem::default(),
        }
    }
}

fn optional(ui: &mut egui::Ui, label: &str, value: &mut Option<i32>, min: i32, max: i32) {
    let mut enabled = value.is_some();
    if ui
        .checkbox(&mut enabled, label)
        .on_hover_text("Unchecked: keep the template value.")
        .changed()
    {
        *value = enabled.then_some(min.max(0));
    }
    if let Some(value) = value {
        number_input(ui, label, value, i32::MIN)
            .on_hover_text(format!("Enter a whole number between {min} and {max}."));
    } else {
        ui.weak("Template");
    }
    ui.end_row();
}

impl ItemEditor {
    pub fn options(&self, weapon: bool) -> ItemOptions {
        match self.mode {
            Mode::Template => ItemOptions::TemplateDefaults,
            Mode::Random => ItemOptions::RandomRoll {
                strength: self.strength,
                quality: self.quality,
                conditions: self.conditions,
            },
            Mode::Custom => {
                let mut settings = self.custom.clone();
                if weapon {
                    settings.resistances = [None; 4];
                } else {
                    settings.weapon = Default::default();
                }
                ItemOptions::Custom {
                    settings: Box::new(settings),
                }
            }
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, weapon: bool) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, Mode::Template, "Template defaults");
            ui.selectable_value(&mut self.mode, Mode::Random, "Random roll");
            ui.selectable_value(&mut self.mode, Mode::Custom, "Custom");
        });
        ui.add_space(4.0);
        match self.mode {
            Mode::Template => {
                ui.label("Use the selected equipment's default values.");
            }
            Mode::Random => {
                ui.horizontal(|ui| {
                    use dd_protocol::LootConditions;
                    ui.label("Loot conditions");
                    egui::ComboBox::from_id_salt("loot-conditions")
                        .selected_text(match self.conditions {
                            LootConditions::CurrentMission => "Current mission",
                            LootConditions::TavernTesting => "Tavern testing",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.conditions,
                                LootConditions::CurrentMission,
                                "Current mission",
                            );
                            ui.selectable_value(
                                &mut self.conditions,
                                LootConditions::TavernTesting,
                                "Tavern testing",
                            );
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Quality");
                    egui::ComboBox::from_id_salt("roll-quality")
                        .selected_text(self.quality.map_or("Any", Quality::label))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.quality, None, "Any");
                            for quality in Quality::ALL {
                                ui.selectable_value(
                                    &mut self.quality,
                                    Some(quality),
                                    quality.label(),
                                );
                            }
                        });
                });
                ui.add(
                    egui::Slider::new(&mut self.strength, 0.01..=2.0)
                        .text("Roll strength")
                        .fixed_decimals(2),
                );
                ui.label("A selected quality searches natural rolls for up to 200 attempts or 15 seconds. Some templates cannot roll every tier.");
                if self.conditions == dd_protocol::LootConditions::TavernTesting {
                    ui.small("Fixed high-level conditions, independent of map and difficulty. Your game settings stay unchanged.");
                }
            }
            Mode::Custom => {
                ui.horizontal(|ui| {
                    ui.label("Quality");
                    egui::ComboBox::from_id_salt("quality")
                        .selected_text(self.custom.quality.map_or("Template", Quality::label))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.custom.quality, None, "Template");
                            for quality in Quality::ALL {
                                ui.selectable_value(
                                    &mut self.custom.quality,
                                    Some(quality),
                                    quality.label(),
                                );
                            }
                        });
                    if ui.small_button("Reset").clicked() {
                        self.custom = CustomItem::default();
                    }
                });
                ui.small("Check a field to override it. Unchecked fields keep template values.");
                egui::CollapsingHeader::new("Upgrades")
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("upgrades").num_columns(2).show(ui, |ui| {
                            optional(
                                ui,
                                "Current level",
                                &mut self.custom.upgrade_level,
                                1,
                                UPGRADE_LIMIT,
                            );
                            optional(
                                ui,
                                "Maximum upgrades",
                                &mut self.custom.max_upgrades,
                                1,
                                UPGRADE_LIMIT,
                            );
                        });
                    });
                egui::CollapsingHeader::new("Hero and defense bonuses")
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::Grid::new("hero-stats").num_columns(2).show(ui, |ui| {
                            for (label, value) in HERO_STATS.iter().zip(&mut self.custom.hero_stats)
                            {
                                optional(ui, label, value, -STAT_LIMIT, STAT_LIMIT);
                            }
                        });
                    });
                egui::CollapsingHeader::new(if weapon { "Weapon bonuses" } else { "Armor resistances" }).default_open(true).show(ui, |ui| {
                    if weapon {
                        ui.small("Bonuses are added to the weapon's base values. Displayed totals and supported effects vary by weapon.");
                    }
                    egui::Grid::new("equipment-stats").num_columns(2).show(ui, |ui| {
                        if weapon {
                            let w = &mut self.custom.weapon;
                            optional(ui, "Damage bonus", &mut w.damage, 0, DAMAGE_LIMIT);
                            optional(ui, "Elemental damage bonus", &mut w.elemental_damage, 0, DAMAGE_LIMIT);
                            optional(ui, "Projectile count bonus", &mut w.projectiles, -127, 128);
                            optional(ui, "Projectile speed bonus", &mut w.projectile_speed, -DAMAGE_LIMIT, DAMAGE_LIMIT);
                            optional(ui, "Attack rate bonus", &mut w.attack_rate, -127, 128);
                            optional(ui, "Ammo bonus", &mut w.clip_ammo, 0, DAMAGE_LIMIT);
                            optional(ui, "Reload bonus", &mut w.reload, -127, 128);
                            optional(ui, "Knockback bonus", &mut w.knockback, -127, 128);
                            optional(ui, "Charge speed bonus", &mut w.charge_speed, -127, 128);
                            optional(ui, "Blocking bonus", &mut w.blocking, -127, 128);
                        } else {
                            for (label, value) in ["Physical %", "Fire %", "Lightning %", "Poison %"].iter().zip(&mut self.custom.resistances) {
                                optional(ui, label, value, 0, 100);
                            }
                        }
                    });
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_equipment_types_omits_inapplicable_overrides() {
        let mut editor = ItemEditor {
            mode: Mode::Custom,
            ..Default::default()
        };
        editor.custom.hero_stats[0] = Some(250);
        editor.custom.resistances[0] = Some(42);
        editor.custom.weapon.damage = Some(12345);
        let ItemOptions::Custom { settings: weapon } = editor.options(true) else {
            panic!()
        };
        assert_eq!(weapon.resistances, [None; 4]);
        assert_eq!(weapon.weapon.damage, Some(12345));
        let ItemOptions::Custom { settings: armor } = editor.options(false) else {
            panic!()
        };
        assert_eq!(armor.weapon, Default::default());
        assert_eq!(armor.resistances[0], Some(42));
        assert_eq!(armor.hero_stats[0], Some(250));
        editor.mode = Mode::Template;
        assert_eq!(editor.options(false), ItemOptions::TemplateDefaults);
    }
}
