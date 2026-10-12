//! Catalog preparation on the IPC worker. Cache reads and package hashing stay outside the game callback.
use dd_catalog::{CatalogKey, CatalogSnapshot, PackageSource};
use dd_protocol::{Command, Request, VERSION};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

// These are integration contracts for the reviewed scripts, not an item list.
const SCRIPT_CONTRACTS: &[(&str, &str)] = &[
    (
        "UDKGame/CookedPCConsole/UDKGame.upk",
        "5f532d3ecf74d85a74be3c6c36ca986a88da1573909ec8525474743479e1e824",
    ),
    (
        "UDKGame/CookedPCConsole/Startup_INT.upk",
        "00656c92c2ad74769210ad914137d93444773057f6d7a3130171e85d42b41568",
    ),
    (
        "UDKGame/CookedPCConsole/Startup_DEU.upk",
        "0022e03f45bc3d979734e12291b82ae6c73eaef02364879999a896e85fec46e5",
    ),
    (
        "UDKGame/CookedPCConsole/Startup_ESN.upk",
        "19798073d12d4fcad9b2963a77fadce0b9dfc63632f3afbc801123e236312a49",
    ),
    (
        "UDKGame/CookedPCConsole/Startup_FRA.upk",
        "1e1404445f54293a4dacf88b4bfd97f921d0c1e71ce1d526d63432685d7280c5",
    ),
    (
        "UDKGame/CookedPCConsole/Startup_ITA.upk",
        "84fe2cc958535997c267437d0052307ab8a7f72e2b9ddf588be3d7fc97d85182",
    ),
];

#[derive(Default)]
pub(crate) struct CatalogState {
    bound: Option<Arc<CatalogSnapshot>>,
    verified: HashMap<PathBuf, (u64, SystemTime, String)>,
    exports: HashMap<(String, String), dd_catalog::ExportRecord>,
}

pub(crate) fn source_path(installation: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Catalog source path is not a relative package path.".into());
    }
    let root = installation
        .join("UDKGame")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let resolved = installation
        .join(relative)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !resolved.starts_with(root)
        || !resolved
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("upk"))
    {
        return Err("Catalog source is outside the installed gameplay packages.".into());
    }
    Ok(resolved)
}

pub(crate) fn source_priority(path: &str) -> (u8, &str) {
    (
        if path.ends_with("/Startup_INT.upk") {
            0
        } else if path.contains("/Startup_") {
            2
        } else {
            1
        },
        path,
    )
}

pub(crate) fn preferred_source(
    snapshot: &CatalogSnapshot,
    path: &str,
) -> Result<PackageSource, String> {
    let entry = snapshot
        .find(path)
        .ok_or("Item is absent from this catalog generation.")?;
    let source = entry
        .source_packages
        .iter()
        .min_by_key(|path| source_priority(path))
        .ok_or("Item has no source package.")?;
    snapshot
        .source(source)
        .cloned()
        .ok_or("Item source has no scan provenance.".into())
}

fn same_key(expected: &CatalogKey, actual: &CatalogKey) -> Result<(), String> {
    expected.validate().map_err(str::to_owned)?;
    if expected != actual {
        return Err("Catalog generation changed; reconnect and inspect the item again.".into());
    }
    Ok(())
}

fn verify_export(
    entry: &dd_catalog::CatalogEntry,
    export: &dd_catalog::ExportRecord,
) -> Result<(), String> {
    if !export.path.eq_ignore_ascii_case(&entry.path) || export.class_name != entry.class_name {
        return Err(
            "Cached item identity/class does not match the source export. Rebuild the catalog."
                .into(),
        );
    }
    if !export.is_archetype || export.is_class_default || export.property_error.is_some() {
        return Err("Selected source export is not a usable item archetype.".into());
    }
    Ok(())
}

impl CatalogState {
    pub(crate) fn commit(&mut self, snapshot: Option<Arc<CatalogSnapshot>>) {
        if let Some(snapshot) = snapshot {
            self.bound = Some(snapshot);
        }
    }

    fn verify_file(&mut self, installation: &Path, source: &PackageSource) -> Result<(), String> {
        let path = source_path(installation, &source.path)?;
        let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        let modified = metadata.modified().map_err(|e| e.to_string())?;
        let stamp = (metadata.len(), modified, source.sha256.clone());
        if self.verified.get(&path) == Some(&stamp) {
            return Ok(());
        }
        if metadata.len() != source.bytes {
            return Err("A package changed since scanning; rebuild the catalog.".into());
        }
        let mut file = File::open(&path).map_err(|e| e.to_string())?;
        let mut digest = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        let after = file.metadata().map_err(|e| e.to_string())?;
        if after.len() != metadata.len()
            || after.modified().map_err(|e| e.to_string())? != modified
            || format!("{:x}", digest.finalize()) != source.sha256
        {
            return Err("A package changed since scanning; rebuild the catalog.".into());
        }
        self.verified.insert(path, stamp);
        Ok(())
    }

    pub(crate) fn prepare(
        &mut self,
        request: &Request,
    ) -> Result<Option<Arc<CatalogSnapshot>>, String> {
        if request.version != VERSION {
            return Ok(None);
        }
        match &request.command {
            Command::BindCatalog { catalog } => {
                catalog.validate().map_err(str::to_owned)?;
                let snapshot = dd_catalog::load_cached(catalog)?;
                same_key(catalog, &snapshot.key)?;
                if !snapshot.issues.is_empty() {
                    return Err(
                        "The scan is incomplete. Review scan errors and rebuild before spawning."
                            .into(),
                    );
                }
                let executable = std::env::current_exe().map_err(|e| e.to_string())?;
                let installation = executable
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .ok_or("Cannot resolve the running game's installation.")?
                    .canonicalize()
                    .map_err(|e| e.to_string())?;
                if Path::new(&snapshot.installation)
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    != installation
                    || snapshot.executable_sha256 != crate::engine::SHA256
                {
                    return Err(
                        "Catalog belongs to a different game installation or executable.".into(),
                    );
                }
                for (path, expected) in SCRIPT_CONTRACTS {
                    let source = snapshot
                        .source(path)
                        .ok_or("Reviewed script package is absent from this catalog.")?;
                    if source.sha256 != *expected {
                        return Err(format!(
                            "Script integration is unverified for {path}. Catalog browsing is available; spawning is disabled."
                        ));
                    }
                    self.verify_file(&installation, source)?;
                }
                Ok(Some(Arc::new(snapshot)))
            }
            Command::InspectItem { catalog, template }
            | Command::Spawn {
                catalog, template, ..
            } => {
                let snapshot = Arc::clone(
                    self.bound
                        .as_ref()
                        .ok_or("Bind the scanned catalog before inspecting or spawning.")?,
                );
                same_key(catalog, &snapshot.key)?;
                let source = preferred_source(&snapshot, template)?;
                self.verify_file(Path::new(&snapshot.installation), &source)?;
                let entry = snapshot
                    .find(template)
                    .ok_or("Unknown item in this catalog.")?;
                if entry.unavailable_reason.is_none() {
                    let key = (source.sha256.clone(), template.to_ascii_lowercase());
                    if !self.exports.contains_key(&key) {
                        let path = source_path(Path::new(&snapshot.installation), &source.path)?;
                        let export = dd_catalog::package::Package::open_streamed(&path)?
                            .inspect_export(template)?;
                        self.verify_file(Path::new(&snapshot.installation), &source)?;
                        if self.exports.len() >= 4096 {
                            self.exports.clear();
                        }
                        self.exports.insert(key.clone(), export);
                    }
                    verify_export(entry, &self.exports[&key])?;
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn another_generation_cannot_authorize_a_request() {
        let key = CatalogKey {
            installation_id: "0".repeat(64),
            generation: "1".repeat(64),
        };
        let mut changed = key.clone();
        changed.generation = "2".repeat(64);
        assert!(same_key(&key, &changed).is_err());
        assert!(same_key(&key, &key).is_ok());
    }
    #[test]
    fn requests_require_explicit_binding() {
        let request = Request::new(
            1,
            Command::InspectItem {
                catalog: CatalogKey {
                    installation_id: "0".repeat(64),
                    generation: "1".repeat(64),
                },
                template: "Package.Item".into(),
            },
        );
        assert!(CatalogState::default().prepare(&request).is_err());
    }

    #[test]
    fn cached_equipment_label_cannot_hide_a_mission_actor_export() {
        let mut entry = dd_catalog::CatalogEntry {
            name: "Fixture".into(),
            path: "Fixture.Present".into(),
            class_name: "UDKGame.HeroEquipment".into(),
            source_packages: vec![],
            kind: dd_catalog::ItemKind::Weapon,
            group: "Fixture".into(),
            variants: vec![],
            equipment_type: Some(0),
            capabilities: Default::default(),
            unavailable_reason: None,
            classification_rule: "fixture".into(),
        };
        let mut export = dd_catalog::ExportRecord {
            path: entry.path.clone(),
            class_name: "DunDefSpecial.DunDefPickupableItem".into(),
            archetype: None,
            is_archetype: true,
            is_class_default: false,
            properties: Default::default(),
            property_error: None,
        };
        assert!(verify_export(&entry, &export).is_err());
        entry.class_name.clone_from(&export.class_name);
        assert!(verify_export(&entry, &export).is_ok());
        export.is_archetype = false;
        assert!(verify_export(&entry, &export).is_err());
    }
}
