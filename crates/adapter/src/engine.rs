//! Bindings for the exact analyzed x86 build. All engine work runs on its game thread.

use dd_protocol::{
    AdapterStatus, CatalogEntry, Command, ErrorCode, ItemInspection, ItemKind, ItemOptions,
    Rejection, Request, Response, SpawnKind, SpawnOptions, SpawnOutcome, VERSION,
    validate_item_options,
};

mod dependencies;
mod equipment;
mod layout;
mod tavern;
mod world;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory,
    LibraryLoader::GetModuleHandleW,
    Threading::{GetCurrentProcess, GetCurrentThreadId},
};

// Recovered from build 25887270 itself; see docs/current-build-validation.md.
pub(crate) const SHA256: &str = "2b43fef8ecf629d72b9a3fc032a65d6bf0298fd983301e97cdbfb45fd6314883";
const NAMES: u32 = 0xd500e4;
const OBJECTS: u32 = 0xd6241c;
const THREAD_ID: u32 = 0xcf021c;
const THREAD_INITIALIZED: u32 = 0xcf0220;
const ENGINE: u32 = 0xd6f368;
const PROCESS_EVENT: u32 = 0x863e0;
#[cfg(target_arch = "x86")]
const ACTOR_PROCESS_EVENT: u32 = 0x210030;
const SPAWN: &str = "UDKGame.DunDefHeroManager.ServerCreateDroppedEquipment";
static STATE: Mutex<Option<Result<Engine>>> = Mutex::new(None);

type Result<T> = std::result::Result<T, String>;

#[derive(Clone)]
struct Object {
    address: u32,
    index: u32,
    outer: u32,
    class: u32,
    identity: [u8; 8],
    name: String,
}

#[derive(Clone, Copy)]
struct Field {
    offset: u32,
    size: u32,
    count: u32,
    mask: u32,
}

struct Engine {
    base: u32,
    references: HashMap<String, Object>,
    fields: HashMap<String, Field>,
    function: u32,
    world: Option<u32>,
    load_failures: HashMap<String, String>,
    catalog: Option<Arc<dd_catalog::CatalogSnapshot>>,
    dependencies: Option<Arc<dd_catalog::dependencies::Dependencies>>,
}

struct Session {
    manager: u32,
    pawn: u32,
    world: u32,
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn reviewed_entry(entry: &CatalogEntry) -> Result<()> {
    require(
        entry.kind != ItemKind::Unknown
            && dd_catalog::rules::reviewed_class(&entry.class_name)
            && dd_catalog::rules::maximum_capabilities(entry.kind, &entry.class_name)
                .template_defaults,
        "This item's concrete class or category has no reviewed spawn lifecycle.",
    )
}

fn read(address: u32, length: usize) -> Result<Vec<u8>> {
    require(
        address >= 0x10000 && length <= 8 * 1024 * 1024,
        "Invalid engine memory range.",
    )?;
    let mut data = vec![0; length];
    let mut count = 0;
    // SAFETY: this is a bounded OS-mediated copy into our allocated buffer, never a raw engine dereference.
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as usize as _,
            data.as_mut_ptr().cast(),
            length,
            &mut count,
        )
    };
    require(
        ok != 0 && count == length,
        "Engine memory is no longer readable. Reconnect after loading.",
    )?;
    Ok(data)
}

fn u32_at(address: u32) -> Result<u32> {
    Ok(u32::from_le_bytes(
        read(address, 4)?.try_into().expect("four bytes"),
    ))
}

fn word(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("four bytes"))
}

fn array(address: u32, limit: u32) -> Result<Vec<u32>> {
    let data = read(address, 12)?;
    let (ptr, count, capacity) = (word(&data, 0), word(&data, 4), word(&data, 8));
    require(
        count <= capacity && capacity <= limit,
        "Invalid engine array.",
    )?;
    if count == 0 {
        return Ok(Vec::new());
    }
    Ok(read(ptr, count as usize * 4)?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect())
}

fn name(entry: u32) -> Result<String> {
    let wide = u32_at(entry + 8)? & 1 != 0;
    let width = if wide { 2 } else { 1 };
    let mut data = Vec::new();
    for offset in (16..1040).step_by(width) {
        let unit = read(entry + offset, width)?;
        if unit.iter().all(|b| *b == 0) {
            return if wide {
                String::from_utf16(
                    &data
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect::<Vec<_>>(),
                )
                .map_err(|e| e.to_string())
            } else {
                String::from_utf8(data).map_err(|e| e.to_string())
            };
        }
        data.extend(unit);
    }
    Err("Unterminated engine name.".into())
}

fn object_path(object: &Object, objects: &HashMap<u32, Object>) -> Result<String> {
    let mut parts = vec![object.name.as_str()];
    let mut outer = object.outer;
    for _ in 0..32 {
        if outer == 0 {
            parts.reverse();
            return Ok(parts.join("."));
        }
        let parent = objects.get(&outer).ok_or("Object outer is absent.")?;
        parts.push(parent.name.as_str());
        outer = parent.outer;
    }
    Err("Invalid object outer chain.".into())
}

impl Engine {
    fn new() -> Result<Self> {
        require(
            usize::BITS == 32,
            "Game integration requires the 32-bit adapter.",
        )?;
        let path = std::env::current_exe().map_err(|e| e.to_string())?;
        require(
            path.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("DunDefGame.exe")),
            "Adapter is not inside Dungeon Defenders.",
        )?;
        let hash = format!(
            "{:x}",
            Sha256::digest(std::fs::read(path).map_err(|e| e.to_string())?)
        );
        require(
            hash == SHA256,
            "Unsupported game executable. No engine calls were enabled.",
        )?;
        // SAFETY: null requests the current process executable module, which remains loaded.
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize as u32;
        let mut engine = Self {
            base,
            references: HashMap::new(),
            fields: HashMap::new(),
            function: 0,
            world: None,
            load_failures: HashMap::new(),
            catalog: None,
            dependencies: None,
        };
        engine.check_thread()?;
        engine.refresh_references()?;
        engine.function = engine.reference(SPAWN)?;
        engine.validate_function()?;
        for (path, kind, size) in FIELDS {
            engine
                .fields
                .insert((*path).into(), engine.reflect_field(path, kind, *size)?);
        }
        equipment::validate(&mut engine)?;
        Ok(engine)
    }

    fn refresh_references(&mut self) -> Result<()> {
        self.check_thread()?;
        let entries = array(self.base + NAMES, 500_000)?;
        require(
            !entries.is_empty() && name(entries[0])? == "None",
            "Name table identity failed.",
        )?;
        let mut names = HashMap::new();
        let mut objects = HashMap::new();
        for (index, address) in array(self.base + OBJECTS, 2_000_000)?
            .into_iter()
            .enumerate()
        {
            if address == 0 {
                continue;
            }
            let data = read(address, 0x3c)?;
            require(
                word(&data, 4) == index as u32,
                "Object table identity failed.",
            )?;
            let name_index = word(&data, 0x2c);
            if let std::collections::hash_map::Entry::Vacant(entry) = names.entry(name_index) {
                let ptr = *entries
                    .get(name_index as usize)
                    .ok_or("Invalid name index.")?;
                entry.insert(name(ptr)?);
            }
            let number = word(&data, 0x30);
            let mut label = names[&name_index].clone();
            if number > 0 {
                label.push_str(&format!("_{}", number - 1));
            }
            objects.insert(
                address,
                Object {
                    address,
                    index: index as u32,
                    outer: word(&data, 0x28),
                    class: word(&data, 0x34),
                    identity: data[0x2c..0x34].try_into().expect("eight bytes"),
                    name: label,
                },
            );
        }
        // Retain metadata, equipment, packages, and archetypes. Player ownership is read afresh.
        let mut references = HashMap::new();
        let equipment_class = objects
            .values()
            .find(|o| {
                o.name == "HeroEquipment"
                    && object_path(o, &objects).as_deref() == Ok("UDKGame.HeroEquipment")
            })
            .ok_or("HeroEquipment class is absent.")?
            .address;
        let mut equipment_classes = HashMap::new();
        for object in objects.values() {
            let class = objects
                .get(&object.class)
                .ok_or("Object class is absent.")?;
            let is_equipment = if let Some(value) = equipment_classes.get(&object.class) {
                *value
            } else {
                let value = Self::class_inherits(object.class, equipment_class)?;
                equipment_classes.insert(object.class, value);
                value
            };
            if class.name == "Class"
                || class.name == "ScriptStruct"
                || class.name == "Enum"
                || class.name.ends_with("Property")
                || class.name == "Function"
                || class.name == "Package"
                || is_equipment
                || u32_at(object.address + 8)? & 0x400 != 0
            {
                references.insert(object_path(object, &objects)?, object.clone());
            }
        }
        self.references = references;
        Ok(())
    }

    fn refresh_session(&mut self) -> Result<Session> {
        let session = self.session()?;
        if self.world.is_some_and(|world| world != session.world) {
            self.load_failures.clear();
            self.refresh_references()?;
            self.validate_function()?;
            equipment::validate(self)?;
        }
        self.world = Some(session.world);
        Ok(session)
    }

    /// Resolve an override on the actual class before falling back to its parents.
    fn method(&self, object: u32, leaf: &str) -> Result<String> {
        self.live_object(object)?;
        let mut class = u32_at(object + 0x34)?;
        for _ in 0..64 {
            if class == 0 {
                break;
            }
            let path = self
                .references
                .iter()
                .find_map(|(path, object)| (object.address == class).then_some(path))
                .ok_or("Class metadata is absent; reconnect after loading.")?;
            let method = format!("{path}.{leaf}");
            if self.references.contains_key(&method) {
                self.reference(&method)?;
                return Ok(method);
            }
            class = u32_at(class + 0x3c)?;
        }
        Err(format!("No supported {leaf} implementation on this item."))
    }

    fn check_thread(&self) -> Result<()> {
        // SAFETY: GetCurrentThreadId has no arguments or ownership requirements.
        let current = unsafe { GetCurrentThreadId() };
        require(
            u32_at(self.base + THREAD_INITIALIZED)? == 1
                && u32_at(self.base + THREAD_ID)? == current,
            "Engine operation was not on the verified game thread.",
        )
    }

    fn reference(&self, path: &str) -> Result<u32> {
        let object = self
            .references
            .get(path)
            .ok_or_else(|| format!("Not loaded: {path}. Reconnect after loading the map."))?;
        self.live_object(object.address)?;
        require(
            u32_at(object.address + 4)? == object.index
                && read(object.address + 0x2c, 8)? == object.identity
                && u32_at(object.address + 0x28)? == object.outer
                && u32_at(object.address + 0x34)? == object.class,
            "A cached engine reference changed. Reconnect.",
        )?;
        Ok(object.address)
    }

    fn live_object(&self, address: u32) -> Result<()> {
        require(address != 0, "Required game object is absent.")?;
        let index = u32_at(address + 4)?;
        let table = read(self.base + OBJECTS, 12)?;
        require(
            index < word(&table, 4) && word(&table, 4) <= 2_000_000,
            "Stale game object.",
        )?;
        require(
            u32_at(word(&table, 0) + index * 4)? == address,
            "Stale game object.",
        )
    }

    fn class_inherits(mut class: u32, expected: u32) -> Result<bool> {
        for _ in 0..64 {
            if class == expected {
                return Ok(true);
            }
            if class == 0 {
                return Ok(false);
            }
            class = u32_at(class + 0x3c)?;
        }
        Err("Invalid class inheritance chain.".into())
    }

    fn is_a(&self, object: u32, class: &str) -> Result<()> {
        self.live_object(object)?;
        require(
            Self::class_inherits(u32_at(object + 0x34)?, self.reference(class)?)?,
            "Unexpected game object class.",
        )
    }

    fn reflect_field(&self, path: &str, kind: &str, size: u32) -> Result<Field> {
        let address = self.reference(path)?;
        require(
            u32_at(address + 0x34)? == self.reference(kind)?,
            &format!("Field type changed: {path}"),
        )?;
        let data = read(address, 0x88)?;
        let field = Field {
            offset: word(&data, 0x64),
            size: word(&data, 0x48),
            count: word(&data, 0x44),
            mask: word(&data, 0x84),
        };
        require(
            field.size == size && field.count > 0 && field.count <= 64 && field.offset < 65536,
            &format!("Field layout changed: {path}"),
        )?;
        if kind == "Core.BoolProperty" {
            require(
                field.mask.is_power_of_two(),
                "Invalid reflected boolean mask.",
            )?;
        }
        Ok(field)
    }

    fn field_address(&self, object: u32, path: &str) -> Result<u32> {
        let (owner, _) = path.rsplit_once('.').ok_or("Invalid property path.")?;
        self.is_a(object, owner)?;
        Ok(object + self.fields.get(path).ok_or("Unresolved field.")?.offset)
    }

    fn value(&self, object: u32, path: &str) -> Result<u32> {
        let address = self.field_address(object, path)?;
        if self.fields[path].size == 1 {
            Ok(read(address, 1)?[0].into())
        } else {
            u32_at(address)
        }
    }

    fn flag(&self, object: u32, path: &str) -> Result<bool> {
        Ok(self.value(object, path)? & self.fields[path].mask != 0)
    }

    fn actor_alive(&self, object: u32) -> Result<()> {
        require(
            !self.flag(object, "Engine.Actor.bDeleteMe")?
                && !self.flag(object, "Engine.Actor.bPendingDelete")?,
            "Actor is being removed. Wait until the map is ready.",
        )
    }

    fn session(&self) -> Result<Session> {
        self.check_thread()?;
        let engine = u32_at(self.base + ENGINE)?;
        self.is_a(engine, "Engine.Engine")?;
        let players = array(
            self.field_address(engine, "Engine.Engine.GamePlayers")?,
            128,
        )?;
        require(players.len() == 1, "Exactly one local player is required.")?;
        let player = players[0];
        self.is_a(player, "Engine.LocalPlayer")?;
        let controller = self.value(player, "Engine.Player.Actor")?;
        self.is_a(controller, "UDKGame.DunDefPlayerController")?;
        self.actor_alive(controller)?;
        require(
            self.value(controller, "Engine.PlayerController.Player")? == player,
            "Local controller ownership is inconsistent.",
        )?;
        require(
            !self.flag(
                controller,
                "UDKGame.DunDefPlayerController.bWaitingForHeroInitialization",
            )? && !self.flag(
                controller,
                "UDKGame.DunDefPlayerController.bCurrentlySwappingHero",
            )?,
            "Wait until the hero finishes loading.",
        )?;
        let hero = self.value(controller, "UDKGame.DunDefPlayerController.myHero")?;
        require(
            self.flag(hero, "UDKGame.DunDefHero.bIsLocalLoadedHero")?
                && self.value(hero, "UDKGame.DunDefHero.ActivePlayer")? == player,
            "Hero is not owned by the active local player.",
        )?;
        let pawn = self.value(controller, "Engine.Controller.Pawn")?;
        self.is_a(pawn, "UDKGame.DunDefPlayer")?;
        self.actor_alive(pawn)?;
        let world = self.value(controller, "Engine.Actor.WorldInfo")?;
        self.actor_alive(world)?;
        require(
            self.value(world, "Engine.WorldInfo.NetMode")? == 0,
            "Spawning is available only in standalone single-player.",
        )?;
        require(
            self.value(pawn, "Engine.Actor.WorldInfo")? == world,
            "Hero and controller are in different worlds.",
        )?;
        let viewport = self.value(engine, "Engine.Engine.GameViewport")?;
        let manager = self.value(viewport, "UDKGame.DunDefViewportClient.TheHeroManager")?;
        require(
            self.flag(manager, "UDKGame.DunDefHeroManager.Initialized")?,
            "Hero manager is not initialized.",
        )?;
        require(
            matches!(
                self.value(
                    manager,
                    "UDKGame.DunDefHeroManagerNative.CurrentMultiplayerMode"
                )?,
                0 | 2
            ),
            "Ranked and unknown modes are unsupported.",
        )?;
        require(
            u32_at(u32_at(manager)? + 0x104)? == self.base + PROCESS_EVENT,
            "Manager dispatcher does not match the verified build.",
        )?;
        Ok(Session {
            manager,
            pawn,
            world,
        })
    }

    fn validate_function(&self) -> Result<()> {
        require(
            u32_at(self.function + 0x34)? == self.reference("Core.Function")?,
            "Spawn entry is not a function.",
        )?;
        let header = read(self.function + 0x90, 0x18)?;
        require(
            word(&header, 0) == 0x824002
                && u16::from_le_bytes([header[4], header[5]]) == 0
                && u16::from_le_bytes([header[0x12], header[0x13]]) == 68
                && u16::from_le_bytes([header[0x14], header[0x15]]) == 64,
            "Spawn function ABI changed.",
        )?;
        for (name, kind, offset, size) in PARAMETERS {
            let path = format!("{SPAWN}.{name}");
            let field = self.reflect_field(&path, kind, *size)?;
            let flags = u32_at(self.reference(&path)? + 0x4c)?;
            require(
                field.offset == *offset && field.count == 1 && flags & 0x80 != 0,
                "Spawn parameter ABI changed.",
            )?;
            if *kind == "Core.BoolProperty" {
                require(field.mask == 1, "Spawn boolean mask changed.")?;
            }
        }
        Ok(())
    }

    fn catalog_flag(&self, object: u32, path: &str) -> Result<bool> {
        let owner = path.rsplit_once('.').ok_or("Invalid catalog property.")?.0;
        self.is_a(object, owner)?;
        let field = self.reflect_field(path, "Core.BoolProperty", 4)?;
        require(
            field.count == 1 && field.mask.is_power_of_two(),
            "Catalog property boolean contract changed.",
        )?;
        Ok(u32_at(object + field.offset)? & field.mask != 0)
    }

    fn validate_classification(&self, entry: &CatalogEntry, template: u32) -> Result<()> {
        require(
            dd_catalog::rules::reviewed_class(&entry.class_name),
            "This item's concrete class has no reviewed spawn lifecycle.",
        )?;
        if entry.kind == ItemKind::WorldPickup {
            require(
                dd_catalog::rules::maximum_capabilities(entry.kind, &entry.class_name)
                    .template_defaults
                    && dd_catalog::rules::maximum_capabilities(entry.kind, &entry.class_name)
                        == entry.capabilities,
                "Cached world-pickup capabilities differ from the reviewed class rules.",
            )?;
            return Ok(());
        }
        let kind = self.value(template, "UDKGame.HeroEquipmentNative.EquipmentType")?;
        let can_equip = self.catalog_flag(template, "UDKGame.HeroEquipment.bCanBeEquipped")?;
        require(
            !self.catalog_flag(template, "UDKGame.HeroEquipment.bIsConsumable")?,
            "Consumable initialization and redemption are not supported by this integration.",
        )?;
        require(
            dd_catalog::rules::category_from_runtime(&entry.class_name, kind as u8, can_equip)
                == entry.kind,
            "Cached category differs from the actual item's class and equipment type.",
        )?;
        let mut properties = std::collections::BTreeMap::new();
        for path in [
            "UDKGame.HeroEquipment.bCantSave",
            "UDKGame.HeroEquipment.bCantPutInItemBox",
            "UDKGame.HeroEquipment.bIsConsumable",
            "UDKGame.HeroEquipmentNative.bForceUseParentTemplate",
            "UDKGame.HeroEquipmentNative.bDisableTheRandomization",
            "UDKGame.HeroEquipment.bUseHighDigitManaTokenValue",
            "UDKGame.HeroEquipment.bPlayerShopForceToMinimumSellWorth",
            "UDKGame.HeroEquipmentNative.UseWeaponCoreStats",
        ] {
            properties.insert(
                path.rsplit('.').next().unwrap().to_owned(),
                serde_json::Value::Bool(self.catalog_flag(template, path)?),
            );
        }
        for (class, path) in [
            (
                "UDKGame.HeroEquipment_Familiar_WithProjectileAI",
                "UDKGame.HeroEquipment_Familiar_WithProjectileAI.bUseFixedShootSpeed",
            ),
            (
                "UDKGame.HeroEquipment_Familiar_Melee",
                "UDKGame.HeroEquipment_Familiar_Melee.bAlsoShootProjectile",
            ),
        ] {
            if self.is_a(template, class).is_ok() {
                properties.insert(
                    path.rsplit('.').next().unwrap().to_owned(),
                    serde_json::Value::Bool(self.catalog_flag(template, path)?),
                );
            }
        }
        if entry.kind == ItemKind::Collectible {
            require(
                kind == 5
                    && dd_catalog::rules::collectible_group(&entry.path, &properties).is_some(),
                "This collectible family has no reviewed initialization rule.",
            )?;
        }
        require(
            dd_catalog::rules::capabilities(entry.kind, &entry.class_name, &properties)
                == entry.capabilities,
            "Cached capabilities differ from the actual template properties; rebuild the catalog.",
        )
    }

    fn validate_template(&self, entry: &CatalogEntry, template: u32) -> Result<()> {
        require(
            entry.unavailable_reason.is_none(),
            entry
                .unavailable_reason
                .as_deref()
                .unwrap_or("Excluded catalog entry."),
        )?;
        self.is_a(template, &entry.class_name)?;
        require(
            u32_at(template + 0x34)? == self.reference(&entry.class_name)?,
            "Template class differs from the reviewed catalog.",
        )?;
        require(
            u32_at(template + 8)? & 0x400 != 0,
            "Choose an archetype, not a live or class-default object.",
        )?;
        require(
            !entry.path.contains("Default__") && !entry.path.starts_with("Transient."),
            "Class defaults and transient objects are unsupported.",
        )?;
        if entry.kind == ItemKind::WorldPickup {
            self.validate_classification(entry, template)?;
            return Ok(());
        }
        self.is_a(template, "UDKGame.HeroEquipment")?;
        self.validate_classification(entry, template)?;
        require(
            self.value(template, "UDKGame.HeroEquipmentNative.EquipmentID1")? == 0
                && self.value(template, "UDKGame.HeroEquipmentNative.EquipmentID2")? == 0,
            "Only unused equipment archetypes with zero item IDs are accepted.",
        )?;
        require(
            !self.flag(
                template,
                "UDKGame.HeroEquipmentNative.bForceUseParentTemplate",
            )?,
            "This template forces its parent; choose the parent template instead.",
        )?;
        require(
            !self.flag(template, "UDKGame.HeroEquipment.bCantSave")?
                && !self.flag(template, "UDKGame.HeroEquipment.bCantPutInItemBox")?,
            "This equipment cannot be stored normally.",
        )?;
        let kind = self.value(template, "UDKGame.HeroEquipmentNative.EquipmentType")?;
        require(
            entry.equipment_type.map(u32::from) == Some(kind),
            "Equipment type differs from the reviewed catalog.",
        )?;
        if entry.kind == ItemKind::Weapon {
            require(
                self.value(template, "UDKGame.HeroEquipment.EquipmentWeaponTemplate")? != 0,
                "Weapon archetype has no weapon template.",
            )?;
            dependencies::validate(self, entry, template)?;
        }
        let drops = array(
            self.field_address(template, "UDKGame.HeroEquipment.DroppedEquipmentTemplates")?,
            64,
        )?;
        require(
            entry.kind != ItemKind::Collectible || !drops.is_empty(),
            "Collectibles require their own verified dropped-equipment template.",
        )?;
        if drops.is_empty() {
            let session = self.session()?;
            let field = self.fields["UDKGame.DunDefHeroManager.DroppedEquipmentTemplates"];
            require(
                kind < field.count,
                "Equipment type has no dropped-item fallback.",
            )?;
            self.is_a(
                u32_at(session.manager + field.offset + kind * 4)?,
                "UDKGame.DunDefDroppedEquipment",
            )?;
        } else {
            for drop in drops {
                self.is_a(drop, "UDKGame.DunDefDroppedEquipment")?;
            }
        }
        Ok(())
    }

    fn check_catalog(&self, key: &dd_catalog::CatalogKey) -> Result<()> {
        key.validate().map_err(str::to_owned)?;
        require(
            self.catalog
                .as_ref()
                .is_some_and(|catalog| catalog.key == *key),
            "Catalog generation changed; reconnect and inspect again.",
        )
    }

    fn inspect(&self, path: &str) -> ItemInspection {
        let entry = self.catalog.as_ref().and_then(|catalog| catalog.find(path));
        let result = (|| -> Result<Option<String>> {
            let session = self.session()?;
            let entry = entry.ok_or("Choose an item from the current scanned catalog.")?;
            if let Some(reason) = &entry.unavailable_reason {
                return Err(reason.clone());
            }
            reviewed_entry(entry)?;
            dd_protocol::validate_grant(path, 1).map_err(str::to_owned)?;
            let template = if self.references.contains_key(path) {
                Some(self.reference(path)?)
            } else {
                None
            };
            if let Some(template) = template {
                self.validate_template(entry, template)?;
            } else {
                if let Some(reason) = self.load_failures.get(path) {
                    return Err(reason.clone());
                }
                world::validate_loader(self)?;
            }
            if entry.kind == ItemKind::WorldPickup {
                world::inspect(self, &session, entry, template)?;
            }
            Ok(template.is_none().then(|| "Assets will be loaded when spawning; unavailable dependencies will reject the request.".into()))
        })();
        let (available, reason) = match result {
            Ok(note) => (true, note),
            Err(reason) => (false, Some(reason)),
        };
        ItemInspection {
            template: path.into(),
            available,
            reason,
            capabilities: entry.map(|item| item.capabilities).unwrap_or_default(),
        }
    }

    fn spawn(
        &mut self,
        template_path: &str,
        quantity: u8,
        options: &SpawnOptions,
        progress: &mut SpawnOutcome,
    ) -> Result<()> {
        dd_protocol::validate_grant(template_path, quantity).map_err(str::to_owned)?;
        let catalog = Arc::clone(
            self.catalog
                .as_ref()
                .ok_or("Bind a scanned catalog first.")?,
        );
        let entry = catalog
            .find(template_path)
            .ok_or("Choose an item from the current scanned catalog.")?;
        if let Some(reason) = &entry.unavailable_reason {
            return Err(reason.clone());
        }
        reviewed_entry(entry)?;
        validate_item_options(entry, options).map_err(str::to_owned)?;
        let session = self.refresh_session()?;
        if entry.kind == ItemKind::WorldPickup {
            // Reject missing mission controllers before activating a cooked package.
            world::inspect(self, &session, entry, None)?;
        }
        let template = if self.references.contains_key(template_path) {
            self.reference(template_path)?
        } else {
            if let Some(reason) = self.load_failures.get(template_path) {
                return Err(reason.clone());
            }
            world::load_template(self, &session, entry).inspect_err(|reason| {
                self.load_failures
                    .insert(template_path.into(), reason.clone());
            })?
        };
        self.validate_template(entry, template)?;
        if matches!(
            options,
            SpawnOptions::Equipment {
                options: ItemOptions::RandomRoll { .. }
            }
        ) {
            require(
                !self.flag(
                    template,
                    "UDKGame.HeroEquipmentNative.bDisableTheRandomization",
                )?,
                "This template disables natural randomization. Use Template defaults or Custom.",
            )?;
        }
        let filtered = options.needs_search();
        require(
            !filtered || quantity == 1,
            "Filtered rolls require one candidate per request.",
        )?;
        let mut ids = Vec::new();
        for index in 0..quantity {
            require(
                crate::bridge::attachment_active(),
                "The attachment owner disconnected; no further pickups were spawned.",
            )?;
            let session = self.session()?;
            if entry.kind == ItemKind::WorldPickup {
                world::inspect(self, &session, entry, Some(template))?;
                world::spawn_one(
                    self,
                    &session,
                    template,
                    entry,
                    index,
                    &mut progress.uncertain,
                )?;
                progress.spawned += 1;
                continue;
            }
            self.validate_function()?;
            let mut params = [0u32; 17];
            params[0] = match options {
                SpawnOptions::Equipment {
                    options: ItemOptions::Custom { settings },
                } => equipment::create(self, template, session.manager, entry, settings)?,
                SpawnOptions::Equipment {
                    options:
                        ItemOptions::RandomRoll {
                            strength,
                            quality,
                            conditions,
                        },
                } if filtered => {
                    let Some(item) = equipment::roll(
                        self,
                        template,
                        session.manager,
                        entry,
                        *strength,
                        *quality,
                        *conditions,
                    )?
                    else {
                        return Ok(());
                    };
                    item
                }
                SpawnOptions::Collectible {
                    stored_mana: Some(_),
                } => {
                    let SpawnOptions::Collectible { stored_mana } = options else {
                        unreachable!()
                    };
                    equipment::create_collectible(self, template, session.manager, *stored_mana)?
                }
                _ => template,
            };
            params[1] = session.pawn;
            params[2] = match options {
                SpawnOptions::Equipment {
                    options: ItemOptions::RandomRoll { strength, .. },
                } if !filtered => strength.to_bits(),
                _ => 0,
            };
            params[3] = u32::from(params[0] == template);
            params[13] = (-2i32) as u32;
            // Source archetypes have placeholder IDs; never delete existing drops based on (0,0).
            params[15] = 1;
            progress.uncertain = true;
            self.dispatch(session.manager, self.function, &mut params)?;
            let dropped = params[16];
            self.is_a(dropped, "UDKGame.DunDefDroppedEquipment")?;
            self.actor_alive(dropped)?;
            require(
                self.value(dropped, "Engine.Actor.WorldInfo")? == session.world,
                "Returned pickup is in a different world.",
            )?;
            let item = self.value(dropped, "UDKGame.DunDefDroppedEquipment.MyEquipmentObject")?;
            self.is_a(item, &entry.class_name)?;
            require(
                u32_at(item + 0x34)? == self.reference(&entry.class_name)?
                    && u32_at(item + 0x38)? == template,
                "Returned equipment does not preserve the selected class and archetype.",
            )?;
            let id = (
                self.value(item, "UDKGame.HeroEquipmentNative.EquipmentID1")?,
                self.value(item, "UDKGame.HeroEquipmentNative.EquipmentID2")?,
            );
            require(
                id != (0, 0) && !ids.contains(&id),
                "Returned equipment does not have a fresh item ID.",
            )?;
            match options {
                SpawnOptions::Equipment {
                    options: ItemOptions::Custom { settings },
                } => equipment::verify(self, item, settings)?,
                SpawnOptions::Equipment {
                    options:
                        ItemOptions::RandomRoll {
                            quality: Some(quality),
                            ..
                        },
                } => {
                    equipment::verify(
                        self,
                        item,
                        &dd_protocol::CustomItem {
                            quality: Some(*quality),
                            ..Default::default()
                        },
                    )?;
                }
                SpawnOptions::Collectible { stored_mana } => {
                    equipment::verify_collectible(self, item, *stored_mana)?
                }
                _ => (),
            }
            ids.push(id);
            progress.uncertain = false;
            progress.spawned += 1;
        }
        Ok(())
    }

    #[cfg(target_arch = "x86")]
    fn dispatch(&self, object: u32, function: u32, params: &mut [u32]) -> Result<()> {
        self.check_thread()?;
        self.live_object(object)?;
        let target = u32_at(u32_at(object)? + 0x104)?;
        require(
            target == self.base + PROCESS_EVENT || target == self.base + ACTOR_PROCESS_EVENT,
            "Object dispatcher does not match the verified build.",
        )?;
        if target == self.base + ACTOR_PROCESS_EVENT {
            self.is_a(object, "Engine.Actor")?;
        }
        // SAFETY: exact image hash, thread, live object, vtable target, reflected function and every
        // parameter's offset/type/size were checked immediately before this call. This x86 ABI was
        // recovered from the shipped caller and callee (ECX=this, three stack args, ret 12).
        unsafe {
            let call: unsafe extern "thiscall" fn(u32, u32, *mut u32, *mut std::ffi::c_void) =
                std::mem::transmute(target as usize);
            call(object, function, params.as_mut_ptr(), std::ptr::null_mut());
        }
        Ok(())
    }

    #[cfg(not(target_arch = "x86"))]
    fn dispatch(&self, _: u32, _: u32, _: &mut [u32]) -> Result<()> {
        Err("Only the x86 adapter can dispatch game functions.".into())
    }
}

pub fn initialize() -> Result<()> {
    // Package loading can pump messages on this thread. Never wait on our own active callback.
    let mut state = STATE
        .try_lock()
        .map_err(|_| "An engine operation is already in progress.")?;
    // Each new attachment refreshes the catalog after map/package changes.
    *state = Some(Engine::new());
    match state.as_ref().expect("initialized") {
        Ok(_) => Ok(()),
        Err(error) => Err(error.clone()),
    }
}

pub fn handle(request: Request, prepared: crate::catalog_runtime::Prepared) -> Response {
    let mut failed_spawn = None;
    let result: Result<AdapterStatus> = (|| {
        require(
            request.version == VERSION,
            "Overlay and adapter versions differ.",
        )?;
        let mut state = STATE.try_lock().map_err(
            |_| "An engine operation is already in progress; no nested action was executed.",
        )?;
        let engine = state
            .as_mut()
            .ok_or("Game adapter has not initialized.")?
            .as_mut()
            .map_err(|error| error.clone())?;
        engine.dependencies = prepared.dependencies;
        let ready = engine.refresh_session();
        let mut status = AdapterStatus {
            pid: std::process::id(),
            bits: usize::BITS,
            diagnostic: false,
            can_give: ready.is_ok() && engine.catalog.is_some(),
            catalog: engine.catalog.as_ref().map(|catalog| catalog.key.clone()),
            roll_matched: None,
            inspection: None,
            spawn: None,
            message: String::new(),
        };
        match request.command {
            Command::Status {} => {
                status.message = match ready {
                    Ok(_) => "Connected to Dungeon Defenders local single-player. Select an item to inspect its availability.".into(),
                    Err(error) => error,
                };
            }
            Command::BindCatalog { catalog } => {
                ready?;
                let prepared = prepared
                    .catalog
                    .ok_or("Catalog was not prepared by the adapter worker.")?;
                require(
                    prepared.key == catalog,
                    "Prepared catalog identity differs.",
                )?;
                engine.catalog = Some(prepared);
                engine.load_failures.clear();
                status.catalog = Some(catalog);
                status.can_give = true;
                status.message = "Scanned catalog bound to this local game session.".into();
            }
            Command::InspectItem { catalog, template } => {
                engine.check_catalog(&catalog)?;
                let inspection = engine.inspect(&template);
                status.message = inspection
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Selected item is available.".into());
                status.inspection = Some(inspection);
            }
            Command::Spawn {
                catalog,
                template,
                quantity,
                options,
            } => {
                ready?;
                engine.check_catalog(&catalog)?;
                let mut outcome = SpawnOutcome {
                    requested: quantity,
                    spawned: 0,
                    uncertain: false,
                    kind: if matches!(options, SpawnOptions::WorldPickup {}) {
                        SpawnKind::WorldPickup
                    } else {
                        SpawnKind::EquipmentPickup
                    },
                };
                if let Err(error) = engine.spawn(&template, quantity, &options, &mut outcome) {
                    failed_spawn = Some(outcome);
                    return Err(format!(
                        "{} pickup(s) confirmed. {error}{}",
                        outcome.spawned,
                        if outcome.uncertain {
                            " A dispatched action could not be confirmed; check the game before retrying."
                        } else {
                            ""
                        }
                    ));
                }
                if options.needs_search() {
                    status.roll_matched = Some(outcome.spawned > 0);
                }
                status.message = if outcome.spawned == 0 {
                    "Roll did not match; nothing spawned.".into()
                } else if outcome.kind == SpawnKind::WorldPickup {
                    format!(
                        "Spawned {} world pickup(s) nearby. Mission effects depend on the active game mode.",
                        outcome.spawned
                    )
                } else {
                    format!(
                        "Spawned {} pickup(s) nearby. Pick them up in the game to equip or store them.",
                        outcome.spawned
                    )
                };
                status.spawn = Some(outcome);
            }
            Command::Shutdown {} => {
                status.can_give = false;
                status.message = "Game adapter disconnected.".into();
            }
        }
        Ok(status)
    })();
    Response {
        version: VERSION,
        id: request.id,
        result: result.map_err(|message| Rejection {
            code: if request.version != VERSION {
                ErrorCode::ProtocolMismatch
            } else {
                ErrorCode::IntegrationUnavailable
            },
            message,
            spawn: failed_spawn,
        }),
    }
}

const PARAMETERS: &[(&str, &str, u32, u32)] = &[
    ("Equipment", "Core.ObjectProperty", 0, 4),
    ("fromActor", "Core.ObjectProperty", 4, 4),
    ("equipmentQuality", "Core.FloatProperty", 8, 4),
    ("useEquipmentObjectAsTemplate", "Core.BoolProperty", 12, 4),
    ("spawnFromDir", "Core.BoolProperty", 16, 4),
    ("dirToSpawn", "Core.StructProperty", 20, 12),
    ("AngleAllowance", "Core.FloatProperty", 32, 4),
    ("useSpawnPos", "Core.BoolProperty", 36, 4),
    ("spawnAtPos", "Core.StructProperty", 40, 12),
    ("remoteUserID", "Core.IntProperty", 52, 4),
    ("backupOldItemNames", "Core.BoolProperty", 56, 4),
    ("bDontCheckForDuplicates", "Core.BoolProperty", 60, 4),
    ("ReturnValue", "Core.ObjectProperty", 64, 4),
];

const FIELDS: &[(&str, &str, u32)] = &[
    ("Engine.Engine.GamePlayers", "Core.ArrayProperty", 12),
    ("Engine.Engine.GameViewport", "Core.ObjectProperty", 4),
    ("Engine.Player.Actor", "Core.ObjectProperty", 4),
    ("Engine.PlayerController.Player", "Core.ObjectProperty", 4),
    ("Engine.Controller.Pawn", "Core.ObjectProperty", 4),
    ("Engine.Actor.WorldInfo", "Core.ObjectProperty", 4),
    ("Engine.Actor.bDeleteMe", "Core.BoolProperty", 4),
    ("Engine.Actor.bPendingDelete", "Core.BoolProperty", 4),
    ("Engine.WorldInfo.NetMode", "Core.ByteProperty", 1),
    (
        "UDKGame.DunDefViewportClient.TheHeroManager",
        "Core.ObjectProperty",
        4,
    ),
    (
        "UDKGame.DunDefPlayerController.myHero",
        "Core.ObjectProperty",
        4,
    ),
    (
        "UDKGame.DunDefPlayerController.bWaitingForHeroInitialization",
        "Core.BoolProperty",
        4,
    ),
    (
        "UDKGame.DunDefPlayerController.bCurrentlySwappingHero",
        "Core.BoolProperty",
        4,
    ),
    ("UDKGame.DunDefHero.ActivePlayer", "Core.ObjectProperty", 4),
    (
        "UDKGame.DunDefHero.bIsLocalLoadedHero",
        "Core.BoolProperty",
        4,
    ),
    (
        "UDKGame.DunDefHeroManager.Initialized",
        "Core.BoolProperty",
        4,
    ),
    (
        "UDKGame.DunDefHeroManagerNative.CurrentMultiplayerMode",
        "Core.ByteProperty",
        1,
    ),
    (
        "UDKGame.DunDefHeroManager.DroppedEquipmentTemplates",
        "Core.ObjectProperty",
        4,
    ),
    (
        "UDKGame.HeroEquipmentNative.EquipmentID1",
        "Core.IntProperty",
        4,
    ),
    (
        "UDKGame.HeroEquipmentNative.EquipmentID2",
        "Core.IntProperty",
        4,
    ),
    (
        "UDKGame.HeroEquipmentNative.EquipmentType",
        "Core.ByteProperty",
        1,
    ),
    (
        "UDKGame.HeroEquipmentNative.bForceUseParentTemplate",
        "Core.BoolProperty",
        4,
    ),
    ("UDKGame.HeroEquipment.bCantSave", "Core.BoolProperty", 4),
    (
        "UDKGame.HeroEquipment.bCantPutInItemBox",
        "Core.BoolProperty",
        4,
    ),
    (
        "UDKGame.HeroEquipment.EquipmentWeaponTemplate",
        "Core.ObjectProperty",
        4,
    ),
    (
        "UDKGame.HeroEquipment.DroppedEquipmentTemplates",
        "Core.ArrayProperty",
        12,
    ),
    (
        "UDKGame.DunDefDroppedEquipment.MyEquipmentObject",
        "Core.ObjectProperty",
        4,
    ),
];
