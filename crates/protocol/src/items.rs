//! Semantic item options. Limits are editor bounds, not normal loot-generation caps.

use serde::{Deserialize, Serialize};

pub const STAT_LIMIT: i32 = 9_999;
pub const DAMAGE_LIMIT: i32 = 1_000_000;
pub const UPGRADE_LIMIT: i32 = 9_999;
pub const HERO_STATS: [&str; 10] = [
    "Hero health",
    "Hero speed",
    "Hero damage",
    "Casting rate",
    "Ability 1",
    "Ability 2",
    "Defense health",
    "Defense attack rate",
    "Defense damage",
    "Defense range",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemOptions {
    #[default]
    TemplateDefaults,
    RandomRoll {
        strength: f32,
        quality: Option<Quality>,
        conditions: LootConditions,
    },
    Custom {
        settings: Box<CustomItem>,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LootConditions {
    #[default]
    CurrentMission,
    TavernTesting,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CustomItem {
    pub quality: Option<Quality>,
    /// Indices 1..=10 in the engine; index zero is NONE and is always preserved.
    pub hero_stats: [Option<i32>; 10],
    /// Match each template's four resistance slots by damage-type class at runtime.
    pub resistances: [Option<i32>; 4],
    pub upgrade_level: Option<i32>,
    pub max_upgrades: Option<i32>,
    pub weapon: WeaponStats,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponStats {
    pub damage: Option<i32>,
    pub elemental_damage: Option<i32>,
    pub projectiles: Option<i32>,
    pub projectile_speed: Option<i32>,
    pub attack_rate: Option<i32>,
    pub clip_ammo: Option<i32>,
    pub reload: Option<i32>,
    pub knockback: Option<i32>,
    pub charge_speed: Option<i32>,
    pub blocking: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Quality {
    Godly = 0,
    Legendary = 1,
    Epic = 2,
    Amazing = 3,
    Powerful = 4,
    Shining = 5,
    Polished = 6,
    Sturdy = 7,
    Solid = 8,
    Stocky = 9,
    Worn = 10,
    Torn = 11,
    Cursed = 12,
    Mythical = 13,
    Transcendent = 14,
    Supreme = 15,
    Ultimate = 16,
    Ultimate93 = 17,
    UltimatePlus = 18,
    UltimatePlusPlus = 19,
}

impl Quality {
    pub const ALL: [Self; 20] = [
        Self::Cursed,
        Self::Torn,
        Self::Worn,
        Self::Stocky,
        Self::Solid,
        Self::Sturdy,
        Self::Polished,
        Self::Shining,
        Self::Powerful,
        Self::Amazing,
        Self::Epic,
        Self::Legendary,
        Self::Godly,
        Self::Mythical,
        Self::Transcendent,
        Self::Supreme,
        Self::Ultimate,
        Self::Ultimate93,
        Self::UltimatePlus,
        Self::UltimatePlusPlus,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Godly => "Godly",
            Self::Legendary => "Legendary",
            Self::Epic => "Epic",
            Self::Amazing => "Amazing",
            Self::Powerful => "Powerful",
            Self::Shining => "Shining",
            Self::Polished => "Polished",
            Self::Sturdy => "Sturdy",
            Self::Solid => "Solid",
            Self::Stocky => "Stocky",
            Self::Worn => "Worn",
            Self::Torn => "Torn",
            Self::Cursed => "Cursed",
            Self::Mythical => "Mythical",
            Self::Transcendent => "Transcendent",
            Self::Supreme => "Supreme",
            Self::Ultimate => "Ultimate",
            Self::Ultimate93 => "Ultimate (93)",
            Self::UltimatePlus => "Ultimate+",
            Self::UltimatePlusPlus => "Ultimate++",
        }
    }
}

fn range(value: Option<i32>, min: i32, max: i32) -> bool {
    value.is_none_or(|v| (min..=max).contains(&v))
}

impl ItemOptions {
    pub fn needs_search(&self) -> bool {
        matches!(
            self,
            Self::RandomRoll {
                quality: Some(_),
                ..
            } | Self::RandomRoll {
                conditions: LootConditions::TavernTesting,
                ..
            }
        )
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::TemplateDefaults => Ok(()),
            Self::RandomRoll { strength, .. } => {
                if strength.is_finite() && (0.01..=2.0).contains(strength) {
                    Ok(())
                } else {
                    Err("Random strength must be between 0.01 and 2.0.")
                }
            }
            Self::Custom { settings } => settings.validate(),
        }
    }
}

impl CustomItem {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self
            .hero_stats
            .iter()
            .all(|v| range(*v, -STAT_LIMIT, STAT_LIMIT))
        {
            return Err("Hero and defense bonuses must be between -9999 and 9999.");
        }
        if !self.resistances.iter().all(|v| range(*v, 0, 100)) {
            return Err("Resistance values must be between 0 and 100.");
        }
        if ![self.upgrade_level, self.max_upgrades]
            .iter()
            .all(|v| range(*v, 1, UPGRADE_LIMIT))
        {
            return Err("Upgrade levels must be between 1 and 9999.");
        }
        if let (Some(level), Some(max)) = (self.upgrade_level, self.max_upgrades)
            && level > max
        {
            return Err("Current upgrade level cannot exceed maximum upgrades.");
        }
        let w = &self.weapon;
        if ![w.damage, w.elemental_damage, w.clip_ammo]
            .iter()
            .all(|v| range(*v, 0, DAMAGE_LIMIT))
            || !range(w.projectile_speed, -DAMAGE_LIMIT, DAMAGE_LIMIT)
        {
            return Err(
                "Weapon damage/ammo must be 0..1000000; projectile speed bonus must be -1000000..1000000.",
            );
        }
        if ![
            w.projectiles,
            w.attack_rate,
            w.reload,
            w.knockback,
            w.charge_speed,
            w.blocking,
        ]
        .iter()
        .all(|v| range(*v, -127, 128))
        {
            return Err(
                "Small weapon bonuses must be between -127 and 128 to survive serialization.",
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_values_that_would_wrap_or_invalidate_the_roll() {
        let mut settings = CustomItem::default();
        settings.weapon.projectiles = Some(129);
        assert!(settings.validate().is_err());
        settings.weapon.projectiles = Some(-127);
        settings.hero_stats[2] = Some(500);
        assert!(settings.validate().is_ok());
        for strength in [0.0, -1.0, 2.01, f32::NAN, f32::INFINITY] {
            assert!(
                ItemOptions::RandomRoll {
                    strength,
                    quality: None,
                    conditions: LootConditions::CurrentMission,
                }
                .validate()
                .is_err()
            );
        }
    }

    #[test]
    fn quality_indices_preserve_the_games_non_ordered_mapping() {
        assert_eq!(Quality::Godly as u8, 0);
        assert_eq!(Quality::Cursed as u8, 12);
        assert_eq!(Quality::Mythical as u8, 13);
        assert_eq!(Quality::UltimatePlusPlus as u8, 19);
    }

    #[test]
    fn tavern_any_quality_is_paced_and_roundtrips() {
        let options = ItemOptions::RandomRoll {
            strength: 1.0,
            quality: None,
            conditions: LootConditions::TavernTesting,
        };
        assert!(options.needs_search());
        assert_eq!(
            serde_json::from_str::<ItemOptions>(&serde_json::to_string(&options).unwrap()).unwrap(),
            options
        );
        assert!(
            !ItemOptions::RandomRoll {
                strength: 1.0,
                quality: None,
                conditions: LootConditions::CurrentMission
            }
            .needs_search()
        );
        assert!(
            serde_json::from_str::<ItemOptions>(
                r#"{"mode":"random_roll","strength":1,"quality":null,"conditions":"invalid"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn filtered_random_roll_roundtrips_and_rejects_unknown_tiers() {
        for quality in [None, Some(Quality::Cursed), Some(Quality::UltimatePlusPlus)] {
            let options = ItemOptions::RandomRoll {
                strength: 1.5,
                quality,
                conditions: LootConditions::CurrentMission,
            };
            assert_eq!(
                serde_json::from_str::<ItemOptions>(&serde_json::to_string(&options).unwrap())
                    .unwrap(),
                options
            );
        }
        assert!(
            serde_json::from_str::<ItemOptions>(
                r#"{"mode":"random_roll","strength":1,"quality":"invalid"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn custom_options_roundtrip_and_reject_unknown_fields() {
        let settings = CustomItem {
            quality: Some(Quality::UltimatePlus),
            max_upgrades: Some(500),
            ..Default::default()
        };
        let options = ItemOptions::Custom {
            settings: Box::new(settings),
        };
        assert_eq!(
            serde_json::from_str::<ItemOptions>(&serde_json::to_string(&options).unwrap()).unwrap(),
            options
        );
        assert!(
            serde_json::from_str::<ItemOptions>(
                r#"{"mode":"custom","settings":{"arbitrary_address":123}}"#
            )
            .is_err()
        );
    }
}
