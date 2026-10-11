//! Small, versioned messages shared by the desktop app and the adapter.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

#[cfg(windows)]
pub mod pipe;

pub const VERSION: u32 = 1;
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
    Give { template: String, quantity: u8 },
    Shutdown {},
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
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rejection {
    pub code: ErrorCode,
    pub message: String,
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
                Command::Give {
                    template: "Package.Group.Item".into(),
                    quantity: 2,
                },
            ),
        )
        .unwrap();
        let request: Request = read_frame(&mut Cursor::new(data)).unwrap();
        assert_eq!(request.id, 42);
        assert!(matches!(request.command, Command::Give { quantity: 2, .. }));
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
}
