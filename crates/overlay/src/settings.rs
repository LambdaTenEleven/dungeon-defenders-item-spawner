use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchLimits {
    pub attempts: u32,
    pub seconds: u32,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            attempts: 200,
            seconds: 15,
        }
    }
}

impl SearchLimits {
    pub fn validate(self) -> Result<(), &'static str> {
        if !(1..=10_000).contains(&self.attempts) {
            return Err("Maximum roll attempts must be between 1 and 10,000.");
        }
        if !(1..=300).contains(&self.seconds) {
            return Err("Search time limit must be between 1 and 300 seconds.");
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.to_string()),
        };
        let limits: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        limits.validate().map_err(str::to_owned)?;
        Ok(limits)
    }

    pub fn save(self, path: &Path) -> Result<(), String> {
        self.validate().map_err(str::to_owned)?;
        let parent = path.parent().ok_or("Settings directory is unavailable.")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(&self).map_err(|error| error.to_string())?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, path).map_err(|error| error.to_string())
    }
}

pub fn path() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|root| PathBuf::from(root).join("DungeonWorkshop/search-settings.json"))
        .ok_or_else(|| "LOCALAPPDATA is unavailable; cannot save search settings.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_and_incomplete_limits_are_rejected() {
        for limits in [
            SearchLimits {
                attempts: 0,
                seconds: 15,
            },
            SearchLimits {
                attempts: 10_001,
                seconds: 15,
            },
            SearchLimits {
                attempts: 200,
                seconds: 0,
            },
            SearchLimits {
                attempts: 200,
                seconds: 301,
            },
        ] {
            assert!(limits.validate().is_err());
        }
        assert!(
            SearchLimits {
                attempts: 10_000,
                seconds: 300
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn settings_survive_restart_and_invalid_save_preserves_last_valid_values() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("dd-search-settings-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(SearchLimits::load(&path).unwrap(), SearchLimits::default());
        let limits = SearchLimits {
            attempts: 700,
            seconds: 45,
        };
        limits.save(&path).unwrap();
        assert_eq!(SearchLimits::load(&path).unwrap(), limits);
        let newer = SearchLimits {
            attempts: 900,
            seconds: 60,
        };
        newer.save(&path).unwrap();
        assert!(
            SearchLimits {
                attempts: 0,
                ..limits
            }
            .save(&path)
            .is_err()
        );
        assert_eq!(SearchLimits::load(&path).unwrap(), newer);
        std::fs::write(&path, "{\"attempts\":0,\"seconds\":15}").unwrap();
        assert!(SearchLimits::load(&path).is_err());
        std::fs::write(&path, "broken").unwrap();
        assert!(SearchLimits::load(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
