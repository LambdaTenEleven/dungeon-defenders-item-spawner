//! Metadata-only, installation-specific cache. Immutable generations are published
//! only after both scanning passes finish; cancellation leaves the old manifest intact.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::{
    CatalogKey, CatalogSnapshot, ClassDefinition, PackageFacts, PackageSource, RULES_VERSION,
    SCANNER_VERSION, SCHEMA_VERSION, ScanProgress, package::Package,
};

const MAX_JSON: u64 = 64 * 1024 * 1024;
const MAX_PACKAGES: usize = 4096;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    scanner_version: u32,
    schema_version: u32,
    graph_digest: String,
    packages: Vec<CachedPackage>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CachedPackage {
    source: PackageSource,
    classes: Vec<ClassDefinition>,
    facts_digest: Option<String>,
    error: Option<String>,
}

pub fn cache_root() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("DungeonWorkshop/cache"))
        .ok_or_else(|| "LOCALAPPDATA is unavailable; cannot locate the catalog cache".into())
}

pub fn scan(
    installation: &Path,
    force: bool,
    cancel: &AtomicBool,
    progress: impl Fn(ScanProgress),
) -> Result<CatalogSnapshot, String> {
    scan_at(installation, &cache_root()?, force, cancel, progress)
}

pub fn scan_at(
    installation: &Path,
    cache: &Path,
    force: bool,
    cancel: &AtomicBool,
    progress: impl Fn(ScanProgress),
) -> Result<CatalogSnapshot, String> {
    cancelled(cancel)?;
    let installation =
        fs::canonicalize(installation).map_err(|error| format!("Installation: {error}"))?;
    let installation_text = installation
        .to_str()
        .ok_or("Installation path is not Unicode")?
        .to_owned();
    let installation_id = digest(installation_text.to_lowercase().as_bytes());
    let directory = cache.join(&installation_id);
    let manifest = if force {
        None
    } else {
        read_json::<Manifest>(&directory.join("manifest.json"))
            .ok()
            .filter(|manifest| {
                manifest.scanner_version == SCANNER_VERSION
                    && manifest.schema_version == SCHEMA_VERSION
            })
    };
    let old: BTreeMap<_, _> = manifest
        .as_ref()
        .map(|manifest| {
            manifest
                .packages
                .iter()
                .map(|package| (package.source.path.clone(), package))
                .collect()
        })
        .unwrap_or_default();
    let paths = discover(&installation.join("UDKGame"), &installation, cancel)?;
    if paths.is_empty() {
        return Err("No .upk packages were found under UDKGame".into());
    }
    let total = paths.len() * 2 + 1;
    let executable = installation.join("Binaries/Win32/DunDefGame.exe");
    let executable_sha256 = hash_file(&executable, cancel)?;
    let mut issues = Vec::new();
    let mut packages = Vec::with_capacity(paths.len());
    let mut retained = BTreeMap::new();
    let mut reused_tables = 0;
    let mut reused_facts = 0;
    for (index, path) in paths.iter().enumerate() {
        cancelled(cancel)?;
        let relative = relative_path(path, &installation)?;
        progress(ScanProgress {
            completed: index,
            total,
            message: format!("Reading package tables: {relative}"),
        });
        let mut source = source_metadata(path, relative.clone())?;
        if let Some(previous) = old.get(&relative).filter(|previous| {
            source.bytes == previous.source.bytes
                && source.modified_ns == previous.source.modified_ns
        }) {
            let facts = previous
                .facts_digest
                .as_ref()
                .and_then(|id| read_facts(&directory, id).ok());
            if previous.error.is_some()
                || facts
                    .as_ref()
                    .is_some_and(|facts| facts.source == previous.source)
            {
                if let Some(facts) = facts {
                    retained.insert(relative, facts);
                }
                packages.push((*previous).clone());
                reused_tables += 1;
                continue;
            }
        }
        match Package::open(path) {
            Ok(mut package) => {
                source.sha256 = digest(package.bytes().ok_or("Scanner package bytes unavailable")?);
                check_source_stable(path, &source)?;
                match package.classes() {
                    Ok(classes) => packages.push(CachedPackage {
                        source,
                        classes,
                        facts_digest: None,
                        error: None,
                    }),
                    Err(error) => packages.push(CachedPackage {
                        source,
                        classes: Vec::new(),
                        facts_digest: None,
                        error: Some(error),
                    }),
                }
            }
            Err(error) => {
                source.sha256 = hash_file(path, cancel)?;
                packages.push(CachedPackage {
                    source,
                    classes: Vec::new(),
                    facts_digest: None,
                    error: Some(error),
                });
            }
        }
    }
    let mut graph = BTreeMap::new();
    let mut conflicting_classes = Vec::new();
    for package in &packages {
        for class in &package.classes {
            if let Some(previous) = graph.insert(class.path.clone(), class.parent.clone())
                && previous != class.parent
            {
                conflicting_classes.push((class.path.clone(), previous, class.parent.clone()));
            }
        }
    }
    for (class, first, second) in conflicting_classes {
        // Cooked maps also contain unrelated UI class variants. Only ambiguous
        // item ancestry can affect candidate discovery or authorize spawning.
        if crate::rules::is_candidate(&class, &graph)
            || first
                .as_ref()
                .is_some_and(|parent| crate::rules::is_candidate(parent, &graph))
            || second
                .as_ref()
                .is_some_and(|parent| crate::rules::is_candidate(parent, &graph))
        {
            issues.push(format!(
                "Conflicting parent definitions for item class {class}"
            ));
        }
    }
    let graph_digest = digest(&serde_json::to_vec(&graph).map_err(|error| error.to_string())?);
    let graph_changed = manifest
        .as_ref()
        .is_none_or(|manifest| manifest.graph_digest != graph_digest);
    let mut facts = Vec::with_capacity(packages.len());
    for (index, package) in packages.iter_mut().enumerate() {
        cancelled(cancel)?;
        progress(ScanProgress {
            completed: paths.len() + index,
            total,
            message: format!("Resolving item metadata: {}", package.source.path),
        });
        if let Some(error) = &package.error {
            issues.push(format!("{}: {error}", package.source.path));
            continue;
        }
        if !graph_changed && let Some(previous) = retained.remove(&package.source.path) {
            facts.push(previous);
            reused_facts += 1;
            continue;
        }
        let path = installation.join(&package.source.path);
        let metadata = source_metadata(&path, package.source.path.clone())?;
        if metadata.bytes != package.source.bytes
            || metadata.modified_ns != package.source.modified_ns
        {
            return Err(format!(
                "Package changed during scanning: {}. Rebuild the catalog.",
                package.source.path
            ));
        }
        match Package::open(&path).and_then(|mut reader| {
            if digest(reader.bytes().ok_or("Scanner package bytes unavailable")?)
                != package.source.sha256
            {
                return Err(
                    "Package contents changed between scanning passes; rebuild the catalog".into(),
                );
            }
            let exports = reader.candidates(&graph)?;
            check_source_stable(&path, &package.source)?;
            Ok(exports)
        }) {
            Ok(exports) => facts.push(PackageFacts {
                source: package.source.clone(),
                classes: package.classes.clone(),
                exports,
            }),
            Err(error) => {
                issues.push(format!("{}: {error}", package.source.path));
                package.error = Some(error);
                package.facts_digest = None;
            }
        }
    }
    cancelled(cancel)?;
    progress(ScanProgress {
        completed: total - 1,
        total,
        message: "Classifying items and publishing catalog".into(),
    });
    let entries = crate::rules::classify(&facts);
    let mut snapshot = CatalogSnapshot {
        key: CatalogKey {
            installation_id,
            generation: String::new(),
        },
        schema_version: SCHEMA_VERSION,
        scanner_version: SCANNER_VERSION,
        rules_version: RULES_VERSION,
        installation: installation_text,
        executable_sha256,
        packages: packages
            .iter()
            .map(|package| package.source.clone())
            .collect(),
        entries,
        issues,
    };
    snapshot.key.generation = generation(&snapshot)?;
    cancelled(cancel)?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    for fact in &facts {
        cancelled(cancel)?;
        let bytes = serde_json::to_vec(fact).map_err(|error| error.to_string())?;
        let id = digest(&bytes);
        immutable_write(&directory.join(format!("facts-{id}.json")), &bytes)?;
        if let Some(package) = packages
            .iter_mut()
            .find(|package| package.source.path == fact.source.path)
        {
            package.facts_digest = Some(id);
        }
    }
    cancelled(cancel)?;
    let bytes = serde_json::to_vec(&snapshot).map_err(|error| error.to_string())?;
    immutable_write(
        &directory.join(format!("catalog-{}.json", snapshot.key.generation)),
        &bytes,
    )?;
    cancelled(cancel)?;
    let manifest = Manifest {
        scanner_version: SCANNER_VERSION,
        schema_version: SCHEMA_VERSION,
        graph_digest,
        packages,
    };
    atomic_write(
        &directory.join("manifest.json"),
        &serde_json::to_vec(&manifest).map_err(|error| error.to_string())?,
    )?;
    progress(ScanProgress {
        completed: total,
        total,
        message: format!(
            "Catalog ready: {} entries, {} package issues; reused {reused_tables}/{} tables and {reused_facts}/{} item records",
            snapshot.entries.len(),
            snapshot.issues.len(),
            paths.len(),
            paths.len()
        ),
    });
    Ok(snapshot)
}

pub fn load_cached(key: &CatalogKey) -> Result<CatalogSnapshot, String> {
    load_cached_at(&cache_root()?, key)
}

pub fn load_cached_at(cache: &Path, key: &CatalogKey) -> Result<CatalogSnapshot, String> {
    validate_digest(&key.installation_id)?;
    validate_digest(&key.generation)?;
    let snapshot: CatalogSnapshot = read_json(
        &cache
            .join(&key.installation_id)
            .join(format!("catalog-{}.json", key.generation)),
    )?;
    if snapshot.key != *key
        || snapshot.schema_version != SCHEMA_VERSION
        || snapshot.scanner_version != SCANNER_VERSION
        || snapshot.rules_version != RULES_VERSION
    {
        return Err("Cached catalog identity or version does not match this application".into());
    }
    if generation(&snapshot)? != key.generation {
        return Err("Cached catalog digest does not match its contents".into());
    }
    if digest(snapshot.installation.to_lowercase().as_bytes()) != key.installation_id {
        return Err("Cached catalog belongs to another installation".into());
    }
    if snapshot.packages.len() > MAX_PACKAGES || snapshot.entries.len() > 100_000 {
        return Err("Cached catalog exceeds supported limits".into());
    }
    validate_digest(&snapshot.executable_sha256)?;
    if snapshot.installation.len() > 8192
        || snapshot.issues.len() > MAX_PACKAGES * 2
        || snapshot.issues.iter().any(|issue| issue.len() > 16384)
    {
        return Err("Cached catalog metadata exceeds supported limits".into());
    }
    let mut sources = BTreeSet::new();
    for source in &snapshot.packages {
        validate_digest(&source.sha256)?;
        let path = Path::new(&source.path);
        if source.path.len() > 8192
            || !sources.insert(source.path.as_str())
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
            || !source.path.starts_with("UDKGame/")
        {
            return Err("Cached catalog contains an invalid source path".into());
        }
    }
    let mut paths = BTreeSet::new();
    for entry in &snapshot.entries {
        if entry.path.is_empty()
            || entry.path.len() > 8192
            || !paths.insert(entry.path.to_ascii_lowercase())
            || entry.class_name.is_empty()
            || entry.class_name.len() > 8192
            || entry.name.len() > 8192
            || entry.group.len() > 1024
            || entry.classification_rule.len() > 4096
            || entry
                .unavailable_reason
                .as_ref()
                .is_some_and(|reason| reason.len() > 16384)
            || entry.variants.len() > 64
            || entry.variants.iter().any(|variant| variant.len() > 1024)
            || entry.source_packages.is_empty()
            || entry.source_packages.len() > MAX_PACKAGES
        {
            return Err("Cached catalog contains ambiguous or oversized item metadata".into());
        }
        let mut entry_sources = BTreeSet::new();
        if entry
            .source_packages
            .iter()
            .any(|source| !sources.contains(source.as_str()) || !entry_sources.insert(source))
        {
            return Err("Cached item refers to missing or duplicated source packages".into());
        }
    }
    Ok(snapshot)
}

fn generation(snapshot: &CatalogSnapshot) -> Result<String, String> {
    let mut value = snapshot.clone();
    value.key.generation.clear();
    Ok(digest(
        &serde_json::to_vec(&value).map_err(|error| error.to_string())?,
    ))
}

fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Catalog scan cancelled; previous cache preserved".into())
    } else {
        Ok(())
    }
}

fn relative_path(path: &Path, installation: &Path) -> Result<String, String> {
    Ok(path
        .strip_prefix(installation)
        .map_err(|_| "Package escapes installation directory")?
        .to_str()
        .ok_or("Package path is not Unicode")?
        .replace('\\', "/"))
}

fn source_metadata(path: &Path, relative: String) -> Result<PackageSource, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    let modified_ns = metadata
        .modified()
        .map_err(|error| error.to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    Ok(PackageSource {
        path: relative,
        sha256: String::new(),
        bytes: metadata.len(),
        modified_ns,
    })
}

fn check_source_stable(path: &Path, expected: &PackageSource) -> Result<(), String> {
    let actual = source_metadata(path, expected.path.clone())?;
    if actual.bytes != expected.bytes || actual.modified_ns != expected.modified_ns {
        return Err(format!(
            "Package changed during scanning: {}. Rebuild the catalog.",
            expected.path
        ));
    }
    Ok(())
}

fn discover(
    directory: &Path,
    installation: &Path,
    cancel: &AtomicBool,
) -> Result<Vec<PathBuf>, String> {
    let package_root = fs::canonicalize(directory).map_err(|error| error.to_string())?;
    if !package_root.starts_with(installation) {
        return Err("UDKGame directory escapes the installation".into());
    }
    let mut pending = vec![package_root.clone()];
    let mut visited = BTreeSet::new();
    let mut files = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        cancelled(cancel)?;
        let directory = fs::canonicalize(directory).map_err(|error| error.to_string())?;
        if !directory.starts_with(&package_root) {
            return Err("Package directory escapes UDKGame".into());
        }
        if !visited.insert(directory.clone()) {
            continue;
        }
        if visited.len() > 16384 {
            return Err("Too many package directories".into());
        }
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = fs::canonicalize(entry.path()).map_err(|error| error.to_string())?;
            if !path.starts_with(&package_root) {
                return Err("Package path escapes UDKGame".into());
            }
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("upk"))
            {
                files.insert(path);
                if files.len() > MAX_PACKAGES {
                    return Err("Too many packages for this scanner".into());
                }
            }
        }
    }
    Ok(files.into_iter().collect())
}

fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        cancelled(cancel)?;
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_digest(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Invalid catalog digest".into());
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&read_bytes(path)?).map_err(|error| error.to_string())
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_JSON {
        return Err("Cache file exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_JSON + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_JSON {
        return Err("Cache file grew beyond size limit".into());
    }
    Ok(bytes)
}

fn read_facts(directory: &Path, id: &str) -> Result<PackageFacts, String> {
    validate_digest(id)?;
    let bytes = read_bytes(&directory.join(format!("facts-{id}.json")))?;
    if digest(&bytes) != id {
        return Err("Cached package facts digest mismatch".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn immutable_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists()
        && fs::metadata(path).map_err(|error| error.to_string())?.len() <= MAX_JSON
        && read_bytes(path)? == bytes
    {
        return Ok(());
    }
    // A corrupted file may be repaired atomically by the scanner.
    atomic_write(path, bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_JSON {
        return Err("Generated cache file exceeds size limit".into());
    }
    let temporary = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/catalog-tests")
                .join(format!(
                    "dd-catalog-test-{}-{}",
                    std::process::id(),
                    TEMP_ID.fetch_add(1, Ordering::Relaxed)
                ));
            fs::create_dir_all(path.join("game/UDKGame")).unwrap();
            fs::create_dir_all(path.join("game/Binaries/Win32")).unwrap();
            fs::write(
                path.join("game/Binaries/Win32/DunDefGame.exe"),
                b"synthetic executable identity",
            )
            .unwrap();
            fs::write(
                path.join("game/UDKGame/Test.upk"),
                b"unsupported synthetic package",
            )
            .unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn partial_scan_cache_reload_and_changed_packages() {
        let fixture = Fixture::new();
        let game = fixture.0.join("game");
        let cache = fixture.0.join("cache");
        let cancel = AtomicBool::new(false);
        let first = scan_at(&game, &cache, false, &cancel, |_| {}).unwrap();
        assert_eq!(first.issues.len(), 1);
        assert_eq!(load_cached_at(&cache, &first.key).unwrap().key, first.key);
        assert_eq!(
            scan_at(&game, &cache, false, &cancel, |_| {}).unwrap().key,
            first.key
        );
        fs::write(game.join("UDKGame/Test.upk"), b"changed malformed package").unwrap();
        let changed = scan_at(&game, &cache, false, &cancel, |_| {}).unwrap();
        assert_ne!(changed.key, first.key);
        assert_eq!(load_cached_at(&cache, &first.key).unwrap().key, first.key);
    }

    #[test]
    fn cancelled_scan_preserves_manifest_and_rejects_path_keys() {
        let fixture = Fixture::new();
        let cache = fixture.0.join("cache");
        let cancel = AtomicBool::new(false);
        let first = scan_at(&fixture.0.join("game"), &cache, false, &cancel, |_| {}).unwrap();
        let manifest_path = cache.join(&first.key.installation_id).join("manifest.json");
        let before = fs::read(&manifest_path).unwrap();
        assert!(
            scan_at(&fixture.0.join("game"), &cache, true, &cancel, |_| cancel
                .store(true, Ordering::Relaxed))
            .is_err()
        );
        assert_eq!(fs::read(manifest_path).unwrap(), before);
        assert!(
            load_cached_at(
                &cache,
                &CatalogKey {
                    installation_id: "../escape".into(),
                    generation: first.key.generation
                }
            )
            .is_err()
        );
    }

    #[test]
    fn altered_catalog_is_rejected() {
        let fixture = Fixture::new();
        let cache = fixture.0.join("cache");
        let first = scan_at(
            &fixture.0.join("game"),
            &cache,
            false,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        let path = cache
            .join(&first.key.installation_id)
            .join(format!("catalog-{}.json", first.key.generation));
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["issues"] = serde_json::json!([]);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            load_cached_at(&cache, &first.key)
                .unwrap_err()
                .contains("digest")
        );
    }

    #[test]
    fn new_class_graph_reconsiders_unchanged_packages_and_removals() {
        let fixture = Fixture::new();
        let game = fixture.0.join("game");
        let cache = fixture.0.join("cache");
        fs::write(
            game.join("UDKGame/Test.upk"),
            crate::package::tests::fixture("Template", "Other", "NewEquipment", false),
        )
        .unwrap();
        let first = scan_at(&game, &cache, false, &AtomicBool::new(false), |_| {}).unwrap();
        assert!(first.entries.is_empty());
        fs::write(
            game.join("UDKGame/Other.upk"),
            crate::package::tests::fixture("NewEquipment", "UDKGame", "HeroEquipment", true),
        )
        .unwrap();
        let second = scan_at(&game, &cache, false, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(second.packages.len(), 2);
        assert_eq!(second.entries.len(), 1);
        assert!(second.entries[0].unavailable_reason.is_some());
        fs::remove_file(game.join("UDKGame/Other.upk")).unwrap();
        let removed = scan_at(&game, &cache, false, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(removed.packages.len(), 1);
        assert!(removed.entries.is_empty());
    }

    #[test]
    fn corrupted_facts_reparse_and_stale_catalog_versions_reject() {
        let fixture = Fixture::new();
        let game = fixture.0.join("game");
        let cache = fixture.0.join("cache");
        fs::write(
            game.join("UDKGame/Test.upk"),
            crate::package::tests::empty_package(),
        )
        .unwrap();
        let first = scan_at(&game, &cache, false, &AtomicBool::new(false), |_| {}).unwrap();
        let directory = cache.join(&first.key.installation_id);
        let manifest: Manifest = read_json(&directory.join("manifest.json")).unwrap();
        let facts = directory.join(format!(
            "facts-{}.json",
            manifest.packages[0].facts_digest.as_ref().unwrap()
        ));
        fs::write(&facts, b"corrupted facts").unwrap();
        let rebuilt = scan_at(&game, &cache, false, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(rebuilt.key, first.key);
        assert!(read_json::<PackageFacts>(&facts).is_ok());
        let path = directory.join(format!("catalog-{}.json", first.key.generation));
        for field in ["schema_version", "scanner_version", "rules_version"] {
            let mut value = serde_json::to_value(&first).unwrap();
            value[field] = serde_json::json!(999);
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(
                load_cached_at(&cache, &first.key)
                    .unwrap_err()
                    .contains("version")
            );
        }
    }

    #[test]
    fn cancellation_before_publish_keeps_previous_manifest() {
        let fixture = Fixture::new();
        let game = fixture.0.join("game");
        let cache = fixture.0.join("cache");
        let first = scan_at(&game, &cache, false, &AtomicBool::new(false), |_| {}).unwrap();
        let manifest_path = cache.join(&first.key.installation_id).join("manifest.json");
        let before = fs::read(&manifest_path).unwrap();
        let cancel = AtomicBool::new(false);
        assert!(
            scan_at(&game, &cache, true, &cancel, |progress| {
                if progress.completed + 1 == progress.total {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
            .is_err()
        );
        assert_eq!(fs::read(&manifest_path).unwrap(), before);
        assert_eq!(load_cached_at(&cache, &first.key).unwrap().key, first.key);
    }
}
