//! Reviewed item metadata shared by the menu and adapter. No game assets are embedded.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Weapon,
    Armor,
    Pet,
    Accessory,
    Collectible,
    WorldPickup,
    Unknown,
}

impl ItemKind {
    pub const ALL: [Self; 7] = [
        Self::Weapon,
        Self::Armor,
        Self::Pet,
        Self::Accessory,
        Self::Collectible,
        Self::WorldPickup,
        Self::Unknown,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Weapon => "Weapons",
            Self::Armor => "Armor",
            Self::Pet => "Pets",
            Self::Accessory => "Accessories",
            Self::Collectible => "Collectibles",
            Self::WorldPickup => "World Pickups",
            Self::Unknown => "Unclassified",
        }
    }
}

/// A field is exposed only when its meaning has been established for this class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PetCapabilities {
    pub damage: bool,
    pub healing: bool,
    pub mana: bool,
    pub boost: bool,
    pub attack_rate: bool,
    pub range: bool,
    pub projectiles: bool,
    pub projectile_speed: bool,
}

/// Static editor capabilities do not imply runtime availability. Inspection must
/// additionally validate the current executable, session, template and lifecycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemCapabilities {
    pub template_defaults: bool,
    pub random_roll: bool,
    pub custom_common: bool,
    pub resistances: bool,
    pub weapon: bool,
    pub pet: Option<PetCapabilities>,
    pub stored_mana: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub name: String,
    pub path: String,
    pub class_name: String,
    pub source_packages: Vec<String>,
    pub kind: ItemKind,
    pub group: String,
    pub variants: Vec<String>,
    pub equipment_type: Option<u8>,
    pub capabilities: ItemCapabilities,
    pub unavailable_reason: Option<String>,
    pub classification_rule: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogKey {
    pub installation_id: String,
    pub generation: String,
}

impl CatalogKey {
    pub fn validate(&self) -> Result<(), &'static str> {
        if [&self.installation_id, &self.generation]
            .iter()
            .all(|value| {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
        {
            Ok(())
        } else {
            Err("Invalid catalog identity; scan this installation again.")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSource {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub modified_ns: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSnapshot {
    pub key: CatalogKey,
    pub schema_version: u32,
    pub scanner_version: u32,
    pub rules_version: u32,
    pub installation: String,
    pub executable_sha256: String,
    pub packages: Vec<PackageSource>,
    pub entries: Vec<CatalogEntry>,
    pub issues: Vec<String>,
}

impl CatalogSnapshot {
    pub fn find(&self, path: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|entry| entry.path == path)
    }
    pub fn source(&self, path: &str) -> Option<&PackageSource> {
        self.packages
            .iter()
            .find(|source| source.path == path.replace('\\', "/"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassDefinition {
    pub path: String,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRecord {
    pub path: String,
    pub class_name: String,
    pub archetype: Option<String>,
    pub is_archetype: bool,
    pub is_class_default: bool,
    pub properties: std::collections::BTreeMap<String, serde_json::Value>,
    pub property_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageFacts {
    pub source: PackageSource,
    pub classes: Vec<ClassDefinition>,
    pub exports: Vec<ExportRecord>,
}

#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub completed: usize,
    pub total: usize,
    pub message: String,
}
