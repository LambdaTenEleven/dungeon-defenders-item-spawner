use crate::number_input::number_input;
use dd_protocol::{
    CustomItem, ItemCapabilities, ItemKind, ItemOptions, Quality, SpawnOptions,
    items::{DAMAGE_LIMIT, HERO_STATS, MANA_TOKEN_LIMIT, STAT_LIMIT, UPGRADE_LIMIT},
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
    profile: Option<(ItemKind, ItemCapabilities)>,
    stored_mana: Option<i32>,
    mode: Mode,
    strength: f32,
    quality: Option<Quality>,
    conditions: dd_protocol::LootConditions,
    custom: CustomItem,
}

impl Default for ItemEditor {
    fn default() -> Self {
        Self {
            profile: None,
            stored_mana: None,
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
    /// A category or capability change starts clean; old fields cannot silently return later.
    pub fn select(&mut self, kind: ItemKind, capabilities: ItemCapabilities) {
        if self.profile.as_ref() != Some(&(kind, capabilities)) {
            *self = Self {
                profile: Some((kind, capabilities)),
                ..Default::default()
            };
        }
    }

    pub fn options(&self, kind: ItemKind, caps: ItemCapabilities) -> SpawnOptions {
        if kind == ItemKind::Collectible {
            return SpawnOptions::Collectible {
                stored_mana: caps.stored_mana.then_some(self.stored_mana).flatten(),
            };
        }
        if kind == ItemKind::WorldPickup {
            return SpawnOptions::WorldPickup {};
        }
        let options = match self.mode {
            Mode::Template => ItemOptions::TemplateDefaults,
            Mode::Random => ItemOptions::RandomRoll {
                strength: self.strength,
                quality: self.quality,
                conditions: self.conditions,
            },
            Mode::Custom => {
                let mut settings = self.custom.clone();
                if !caps.resistances {
                    settings.resistances = [None; 4];
                }
                if !caps.weapon {
                    settings.weapon = Default::default();
                }
                let pet = &mut settings.pet;
                match caps.pet {
                    None => *pet = Default::default(),
                    Some(c) => {
                        if !c.damage {
                            pet.damage = None;
                        }
                        if !c.healing {
                            pet.healing = None;
                        }
                        if !c.mana {
                            pet.mana = None;
                        }
                        if !c.boost {
                            pet.boost = None;
                        }
                        if !c.attack_rate {
                            pet.attack_rate = None;
                        }
                        if !c.range {
                            pet.range = None;
                        }
                        if !c.projectiles {
                            pet.projectiles = None;
                        }
                        if !c.projectile_speed {
                            pet.projectile_speed = None;
                        }
                    }
                }
                ItemOptions::Custom {
                    settings: Box::new(settings),
                }
            }
        };
        SpawnOptions::Equipment { options }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, kind: ItemKind, caps: ItemCapabilities) {
        self.select(kind, caps);
        if kind == ItemKind::Unknown {
            ui.weak("This item type has no verified spawn options.");
            return;
        }
        if kind == ItemKind::Collectible {
            ui.label("Create the collectible's packaged pickup. Collect and redeem it normally in the game.");
            if caps.stored_mana {
                egui::Grid::new("stored-mana")
                    .num_columns(2)
                    .show(ui, |ui| {
                        optional(
                            ui,
                            "Stored mana",
                            &mut self.stored_mana,
                            1,
                            MANA_TOKEN_LIMIT,
                        );
                    });
            }
            return;
        }
        if kind == ItemKind::WorldPickup {
            ui.label("Create the selected physical pickup near your hero.");
            ui.small("Mission pickups keep their packaged behavior. Outside their mission, spawning does not grant mission progress; unsupported contexts are unavailable.");
            return;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, Mode::Template, "Template defaults");
            ui.add_enabled_ui(caps.random_roll, |ui| {
                ui.selectable_value(&mut self.mode, Mode::Random, "Random roll");
            });
            ui.add_enabled_ui(caps.custom_common, |ui| {
                ui.selectable_value(&mut self.mode, Mode::Custom, "Custom");
            });
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
                ui.label("A selected quality searches natural rolls until a match or a search limit. Some templates cannot roll every tier.");
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
                if caps.weapon {
                    egui::CollapsingHeader::new("Weapon bonuses").default_open(true).show(ui, |ui| {
                        ui.small("Bonuses are added to the weapon's base values. Displayed totals and supported effects vary by weapon.");
                        egui::Grid::new("weapon-stats").num_columns(2).show(ui, |ui| {
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
                        });
                    });
                }
                if let Some(caps) = caps.pet {
                    egui::CollapsingHeader::new("Pet abilities").default_open(true).show(ui, |ui| {
                        ui.small("These fields use this pet family's verified stat meanings. Unchecked values stay at their defaults.");
                        egui::Grid::new("pet-stats").num_columns(2).show(ui, |ui| {
                            let p = &mut self.custom.pet;
                            if caps.damage { optional(ui, "Damage bonus", &mut p.damage, 0, DAMAGE_LIMIT); }
                            if caps.healing { optional(ui, "Healing bonus", &mut p.healing, 0, DAMAGE_LIMIT); }
                            if caps.mana { optional(ui, "Mana generation bonus", &mut p.mana, 0, DAMAGE_LIMIT); }
                            if caps.boost { optional(ui, "Boost bonus", &mut p.boost, 0, 128); }
                            if caps.attack_rate { optional(ui, "Attack rate bonus", &mut p.attack_rate, -127, 128); }
                            if caps.range { optional(ui, "Range bonus", &mut p.range, 0, DAMAGE_LIMIT); }
                            if caps.projectiles { optional(ui, "Projectile count bonus", &mut p.projectiles, -127, 128); }
                            if caps.projectile_speed { optional(ui, "Projectile speed bonus", &mut p.projectile_speed, -DAMAGE_LIMIT, DAMAGE_LIMIT); }
                        });
                    });
                }
                if caps.resistances {
                    egui::CollapsingHeader::new("Resistances")
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::Grid::new("resistance-stats")
                                .num_columns(2)
                                .show(ui, |ui| {
                                    for (label, value) in
                                        ["Physical %", "Fire %", "Lightning %", "Poison %"]
                                            .iter()
                                            .zip(&mut self.custom.resistances)
                                    {
                                        optional(ui, label, value, 0, 100);
                                    }
                                });
                        });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: ItemKind) -> dd_catalog::CatalogEntry {
        crate::catalog::test_entry(kind)
    }

    #[test]
    fn changing_category_discards_old_fields_and_mode() {
        let mut editor = ItemEditor::default();
        let weapon = entry(ItemKind::Weapon);
        let collectible = entry(ItemKind::Collectible);
        editor.select(weapon.kind, weapon.capabilities);
        editor.mode = Mode::Custom;
        editor.custom.hero_stats[0] = Some(250);
        editor.custom.weapon.damage = Some(12345);
        editor.select(collectible.kind, collectible.capabilities);
        assert!(matches!(
            editor.options(collectible.kind, collectible.capabilities),
            SpawnOptions::Collectible { stored_mana: None }
        ));
        editor.select(weapon.kind, weapon.capabilities);
        assert_eq!(editor.custom, CustomItem::default());
        assert_eq!(
            editor.options(weapon.kind, weapon.capabilities),
            SpawnOptions::Equipment {
                options: ItemOptions::TemplateDefaults
            }
        );
    }

    #[test]
    fn equipment_options_omit_unavailable_overrides() {
        let mut editor = ItemEditor {
            mode: Mode::Custom,
            ..Default::default()
        };
        editor.custom.hero_stats[0] = Some(250);
        editor.custom.weapon.damage = Some(12345);
        editor.custom.resistances[0] = Some(42);
        editor.custom.pet.damage = Some(99);
        let armor = entry(ItemKind::Armor);
        let SpawnOptions::Equipment {
            options: ItemOptions::Custom { settings },
        } = editor.options(armor.kind, armor.capabilities)
        else {
            panic!()
        };
        assert_eq!(settings.weapon, Default::default());
        assert_eq!(settings.pet, Default::default());
        assert_eq!(settings.hero_stats[0], Some(250));
        let weapon = entry(ItemKind::Weapon);
        let SpawnOptions::Equipment {
            options: ItemOptions::Custom { settings },
        } = editor.options(weapon.kind, weapon.capabilities)
        else {
            panic!()
        };
        assert_eq!(settings.resistances, [None; 4]);
        assert_eq!(settings.weapon.damage, Some(12345));
    }

    #[test]
    fn changing_pet_family_clears_ability_overrides() {
        let mut healer = entry(ItemKind::Pet);
        healer.capabilities.pet = Some(dd_protocol::PetCapabilities {
            healing: true,
            ..Default::default()
        });
        let mut guardian = entry(ItemKind::Pet);
        guardian.capabilities.pet = Some(dd_protocol::PetCapabilities {
            boost: true,
            ..Default::default()
        });
        let mut editor = ItemEditor::default();
        editor.select(healer.kind, healer.capabilities);
        editor.mode = Mode::Custom;
        editor.custom.pet.healing = Some(1000);
        editor.select(guardian.kind, guardian.capabilities);
        assert_eq!(editor.custom.pet, Default::default());
        assert!(matches!(
            editor.options(guardian.kind, guardian.capabilities),
            SpawnOptions::Equipment {
                options: ItemOptions::TemplateDefaults
            }
        ));
    }

    #[test]
    fn world_pickup_options_never_include_equipment_or_stored_mana() {
        let mut editor = ItemEditor::default();
        editor.custom.weapon.damage = Some(12345);
        editor.stored_mana = Some(1000);
        let world = entry(ItemKind::WorldPickup);
        assert_eq!(
            editor.options(world.kind, world.capabilities),
            SpawnOptions::WorldPickup {}
        );
    }
}
