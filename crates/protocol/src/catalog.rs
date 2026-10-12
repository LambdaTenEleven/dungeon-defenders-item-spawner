//! Option validation over discovered item metadata.
use crate::{ItemOptions, SpawnOptions};
pub use dd_catalog::{
    CatalogEntry, CatalogKey, CatalogSnapshot, ItemCapabilities, ItemKind, PetCapabilities,
};

/// Definition-level validation shared by the UI and adapter, independently of
/// the runtime's stricter reflected-field and lifecycle checks.
pub fn validate_item_options(
    item: &CatalogEntry,
    options: &SpawnOptions,
) -> Result<(), &'static str> {
    options.validate()?;
    if item.unavailable_reason.is_some() {
        return Err("This catalog entry is excluded; inspect it for the specific reason.");
    }
    let c = item.capabilities;
    if !c.template_defaults {
        return Err("This item has no supported spawning lifecycle.");
    }
    match (item.kind, options) {
        (ItemKind::Collectible, SpawnOptions::Collectible { stored_mana }) => {
            if stored_mana.is_some() && !c.stored_mana {
                return Err("Stored mana is only supported by inventory mana tokens.");
            }
        }
        (ItemKind::WorldPickup, SpawnOptions::WorldPickup {}) => {}
        (
            ItemKind::Weapon | ItemKind::Armor | ItemKind::Pet | ItemKind::Accessory,
            SpawnOptions::Equipment { options },
        ) => match options {
            ItemOptions::TemplateDefaults if !c.template_defaults => {
                return Err("Template-default spawning is not supported for this item.");
            }
            ItemOptions::RandomRoll { .. } if !c.random_roll => {
                return Err("Random rolls are not supported for this item.");
            }
            ItemOptions::Custom { settings } => {
                if !c.custom_common {
                    return Err("Custom equipment settings are not supported for this item.");
                }
                if !c.resistances && settings.resistances.iter().any(Option::is_some) {
                    return Err("Resistance overrides are not supported for this item.");
                }
                if !c.weapon && settings.weapon != Default::default() {
                    return Err("Weapon overrides are only supported by weapons.");
                }
                let p = c.pet.unwrap_or_default();
                let requested = &settings.pet;
                for (enabled, value) in [
                    (p.damage, requested.damage),
                    (p.healing, requested.healing),
                    (p.mana, requested.mana),
                    (p.boost, requested.boost),
                    (p.attack_rate, requested.attack_rate),
                    (p.range, requested.range),
                    (p.projectiles, requested.projectiles),
                    (p.projectile_speed, requested.projectile_speed),
                ] {
                    if !enabled && value.is_some() {
                        return Err("A requested pet override is unsupported by this pet family.");
                    }
                }
            }
            _ => {}
        },
        _ => return Err("Spawn options do not match the selected item's category."),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(kind: ItemKind) -> CatalogEntry {
        CatalogEntry {
            name: "Synthetic".into(),
            path: "Fixture.Item".into(),
            class_name: "Fixture.Class".into(),
            source_packages: vec!["UDKGame/Fixture.upk".into()],
            kind,
            group: "Fixture".into(),
            variants: vec![],
            equipment_type: None,
            capabilities: ItemCapabilities {
                template_defaults: true,
                random_roll: true,
                custom_common: true,
                ..Default::default()
            },
            unavailable_reason: None,
            classification_rule: "fixture".into(),
        }
    }
    #[test]
    fn unknown_category_cannot_acquire_permission_from_forged_capabilities() {
        let item = entry(ItemKind::Unknown);
        for options in [
            SpawnOptions::Equipment {
                options: ItemOptions::TemplateDefaults,
            },
            SpawnOptions::Collectible { stored_mana: None },
            SpawnOptions::WorldPickup {},
        ] {
            assert!(validate_item_options(&item, &options).is_err());
        }
    }

    #[test]
    fn zero_capabilities_cannot_enable_collectible_or_world_defaults() {
        for (kind, options) in [
            (
                ItemKind::Collectible,
                SpawnOptions::Collectible { stored_mana: None },
            ),
            (ItemKind::WorldPickup, SpawnOptions::WorldPickup {}),
        ] {
            let mut item = entry(kind);
            item.capabilities = Default::default();
            assert!(validate_item_options(&item, &options).is_err());
        }
    }
    #[test]
    fn options_do_not_cross_item_categories() {
        let options = SpawnOptions::Equipment {
            options: ItemOptions::Custom {
                settings: Default::default(),
            },
        };
        assert!(validate_item_options(&entry(ItemKind::Weapon), &options).is_ok());
        for kind in [ItemKind::Collectible, ItemKind::WorldPickup] {
            assert!(validate_item_options(&entry(kind), &options).is_err());
        }
        let mut item = entry(ItemKind::Weapon);
        item.unavailable_reason = Some("Unresolved class".into());
        assert!(validate_item_options(&item, &options).is_err());
    }
    #[test]
    fn pet_fields_and_mana_values_require_the_specific_capability() {
        let mut settings = crate::CustomItem::default();
        settings.pet.healing = Some(123);
        let options = SpawnOptions::Equipment {
            options: ItemOptions::Custom {
                settings: Box::new(settings),
            },
        };
        let mut item = entry(ItemKind::Pet);
        assert!(validate_item_options(&item, &options).is_err());
        item.capabilities.pet = Some(PetCapabilities {
            healing: true,
            ..Default::default()
        });
        assert!(validate_item_options(&item, &options).is_ok());
        let mut item = entry(ItemKind::Collectible);
        let options = SpawnOptions::Collectible {
            stored_mana: Some(123),
        };
        assert!(validate_item_options(&item, &options).is_err());
        item.capabilities.stored_mana = true;
        assert!(validate_item_options(&item, &options).is_ok());
    }
}
