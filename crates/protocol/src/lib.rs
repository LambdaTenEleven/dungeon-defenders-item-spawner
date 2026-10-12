//! Small, versioned messages shared by the desktop app and the adapter.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub mod items;
pub use items::{CustomItem, ItemOptions, LootConditions, PetStats, Quality, WeaponStats};
pub mod catalog;
pub use catalog::{
    CatalogEntry, CatalogKey, CatalogSnapshot, ItemCapabilities, ItemKind, PetCapabilities,
    validate_item_options,
};

#[cfg(windows)]
pub mod pipe;

pub const VERSION: u32 = 6;
pub const MAX_FRAME_SIZE: usize = 16 * 1024;
pub const MAX_QUANTITY: u8 = 10;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub id: u64,
    pub command: Command,
}

impl Request {
    pub fn new(id: u64, command: Command) -> Self {
        Self {
            version: VERSION,
            id,
            command,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Status {},
    BindCatalog {
        catalog: CatalogKey,
    },
    InspectItem {
        catalog: CatalogKey,
        template: String,
    },
    Spawn {
        catalog: CatalogKey,
        template: String,
        quantity: u8,
        options: SpawnOptions,
    },
    Shutdown {},
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpawnOptions {
    Equipment { options: ItemOptions },
    Collectible { stored_mana: Option<i32> },
    WorldPickup {},
}

impl SpawnOptions {
    pub fn needs_search(&self) -> bool {
        matches!(self, Self::Equipment { options } if options.needs_search())
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Equipment { options } => options.validate(),
            Self::Collectible { stored_mana }
                if stored_mana
                    .is_some_and(|mana| !(1..=items::MANA_TOKEN_LIMIT).contains(&mana)) =>
            {
                Err("Stored mana must be between 1 and 2000000000.")
            }
            Self::Collectible { .. } | Self::WorldPickup {} => Ok(()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub id: u64,
    pub result: Result<AdapterStatus, Rejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterStatus {
    pub pid: u32,
    pub bits: u32,
    pub diagnostic: bool,
    pub can_give: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<CatalogKey>,
    /// Set for a paced single-candidate roll; false confirms nothing was spawned.
    pub roll_matched: Option<bool>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspection: Option<ItemInspection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn: Option<SpawnOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemInspection {
    pub template: String,
    pub available: bool,
    pub reason: Option<String>,
    pub capabilities: ItemCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnOutcome {
    pub requested: u8,
    pub spawned: u8,
    pub kind: SpawnKind,
    /// The last dispatched action could not be confirmed; callers must not retry.
    #[serde(default)]
    pub uncertain: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnKind {
    EquipmentPickup,
    WorldPickup,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rejection {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn: Option<SpawnOutcome>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    ProtocolMismatch,
    InvalidRequest,
    IntegrationUnavailable,
}

pub fn validate_grant(template: &str, quantity: u8) -> Result<(), &'static str> {
    if !(1..=MAX_QUANTITY).contains(&quantity) {
        return Err("Quantity must be between 1 and 10.");
    }
    if template.len() > 256
        || !template.contains('.')
        || template.split('.').any(str::is_empty)
        || !template
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
    {
        return Err("Use a template path such as Package.Group.Item (maximum 256 characters).");
    }
    Ok(())
}

pub fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> io::Result<()> {
    let payload = serde_json::to_vec(value).map_err(io::Error::other)?;
    if payload.is_empty() || payload.len() > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Message is too large.",
        ));
    }
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(&payload)
}

pub fn read_frame<T: for<'a> Deserialize<'a>>(reader: &mut impl Read) -> io::Result<T> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid message length.",
        ));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
fn test_catalog_key() -> CatalogKey {
    CatalogKey {
        installation_id: "0".repeat(64),
        generation: "1".repeat(64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn rejects_oversized_frame_before_reading_payload() {
        let data = ((MAX_FRAME_SIZE + 1) as u32).to_le_bytes();
        assert_eq!(
            read_frame::<Request>(&mut Cursor::new(data))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn rejects_truncated_message() {
        assert_eq!(
            read_frame::<Request>(&mut Cursor::new([4, 0, 0, 0, b'{']))
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn request_survives_framing() {
        let mut data = Vec::new();
        write_frame(
            &mut data,
            &Request::new(
                42,
                Command::Spawn {
                    catalog: test_catalog_key(),
                    template: "Package.Group.Item".into(),
                    quantity: 2,
                    options: SpawnOptions::Equipment {
                        options: ItemOptions::default(),
                    },
                },
            ),
        )
        .unwrap();
        let request: Request = read_frame(&mut Cursor::new(data)).unwrap();
        assert_eq!(request.id, 42);
        assert!(matches!(
            request.command,
            Command::Spawn { quantity: 2, .. }
        ));
    }

    #[test]
    fn rejects_invalid_templates_and_counts() {
        for path in ["", "A..B", ".B", "A/B", "A.B\n", "A.B;quit"] {
            assert!(validate_grant(path, 1).is_err());
        }
        assert!(validate_grant("A.B", 0).is_err());
        assert!(validate_grant("A.B", 11).is_err());
        assert!(
            validate_grant(
                "DunDef_CDT_Items.SwordOfEmbers.HeroEquipment_SwordOfEmbers",
                1
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_unknown_command_fields() {
        assert!(
            serde_json::from_str::<Command>(r#"{"type":"status","execute":"anything"}"#).is_err()
        );
    }

    #[test]
    fn version_six_rejects_legacy_and_cross_category_wire_shapes() {
        assert_eq!(VERSION, 6);
        assert!(serde_json::from_str::<Command>(r#"{"type":"give","template":"A.B","quantity":1,"options":{"mode":"template_defaults"}}"#).is_err());
        assert!(
            serde_json::from_str::<SpawnOptions>(
                r#"{"kind":"collectible","stored_mana":1,"quality":"godly"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<SpawnOptions>(
                r#"{"kind":"world_pickup","options":{"mode":"template_defaults"}}"#
            )
            .is_err()
        );
        for stored_mana in [Some(0), Some(-1), Some(items::MANA_TOKEN_LIMIT + 1)] {
            assert!(
                SpawnOptions::Collectible { stored_mana }
                    .validate()
                    .is_err()
            );
        }
        let command = Command::InspectItem {
            catalog: test_catalog_key(),
            template: "A.B".into(),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &Request::new(17, command)).unwrap();
        assert!(matches!(
            read_frame::<Request>(&mut Cursor::new(bytes))
                .unwrap()
                .command,
            Command::InspectItem { .. }
        ));
    }

    #[test]
    fn partial_failures_preserve_counts_and_uncertainty() {
        let response = Response {
            version: VERSION,
            id: 8,
            result: Err(Rejection {
                code: ErrorCode::IntegrationUnavailable,
                message: "Last dispatched pickup could not be confirmed.".into(),
                spawn: Some(SpawnOutcome {
                    requested: 4,
                    spawned: 2,
                    kind: SpawnKind::EquipmentPickup,
                    uncertain: true,
                }),
            }),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &response).unwrap();
        let rejection = read_frame::<Response>(&mut Cursor::new(bytes))
            .unwrap()
            .result
            .unwrap_err();
        let outcome = rejection.spawn.unwrap();
        assert_eq!(outcome.spawned, 2);
        assert_eq!(outcome.requested, 4);
        assert!(outcome.uncertain);
    }
}
