//! Allowlisted package loading and physical pickups through the current build's
//! checked native/reflected entry points. Inspection never loads or spawns.

use super::{Engine, Result, Session, array, read, require, u32_at, word};
use dd_protocol::{CatalogEntry, ItemKind};
use std::{os::windows::ffi::OsStrExt, path::PathBuf};

const LOAD: &str = "Core.Object.DynamicLoadObject";
const SPAWN: &str = "Engine.Actor.Spawn";
const INIT_MESH: &str = "DunDefSpecial.DunDefPickupableItem.InitMesh";
const LOAD_PACKAGE_RVA: u32 = 0xc7c60;

#[derive(Debug, PartialEq, Eq)]
enum Context {
    Regular,
    Golden,
    Delivery,
}

fn context_requirement(class: &str) -> Result<Context> {
    match class {
        "UDKGame.DunDefManaToken" | "UDKGame.DunDefBuffToken" => Ok(Context::Regular),
        "DunDefSpecial.DunDefManaToken_Golden" => Ok(Context::Golden),
        "DunDefSpecial.DunDefPickupableItem" => Ok(Context::Delivery),
        "DunDefSpecial.CTF_Flag" => Err(
            "Flag spawning requires verified CTF team, controller, and flag-base initialization. This lifecycle is not yet supported, including outside CTF maps.".into(),
        ),
        _ => Err("This world-pickup class has no reviewed lifecycle.".into()),
    }
}

struct Parameter {
    name: &'static str,
    kind: &'static str,
    offset: u32,
    size: u32,
    flags: u32,
    target: Option<&'static str>,
    meta_class: Option<&'static str>,
}

fn validate_function(
    engine: &Engine,
    path: &str,
    flags: u32,
    size: u16,
    return_offset: u16,
    parameters: &[Parameter],
) -> Result<u32> {
    let function = engine.reference(path)?;
    let header = read(function + 0x90, 0x18)?;
    require(
        u32_at(function + 0x34)? == engine.reference("Core.Function")?
            && word(&header, 0) == flags
            && u16::from_le_bytes([header[4], header[5]]) == 0
            && u16::from_le_bytes([header[0x12], header[0x13]]) == size
            && u16::from_le_bytes([header[0x14], header[0x15]]) == return_offset,
        &format!("Function ABI changed: {path}"),
    )?;
    for parameter in parameters {
        let property = format!("{path}.{}", parameter.name);
        let field = engine.reflect_field(&property, parameter.kind, parameter.size)?;
        let address = engine.reference(&property)?;
        require(
            field.offset == parameter.offset
                && field.count == 1
                && u32_at(address + 0x4c)? == parameter.flags
                && u32_at(address + 0x50)? == 0,
            &format!("Parameter ABI changed: {property}"),
        )?;
        if parameter.kind == "Core.BoolProperty" {
            require(field.mask == 1, "Pickup boolean mask changed.")?;
        }
        if let Some(target) = parameter.target {
            require(
                u32_at(address + 0x84)? == engine.reference(target)?,
                "Pickup parameter target changed.",
            )?;
        }
        if let Some(target) = parameter.meta_class {
            require(
                u32_at(address + 0x88)? == engine.reference(target)?,
                "Pickup class-parameter constraint changed.",
            )?;
        }
    }
    Ok(function)
}

pub(super) fn validate_loader(engine: &Engine) -> Result<()> {
    validate_function(engine, LOAD, 0x4026401, 24, 20, LOAD_PARAMETERS).map(|_| ())
}

fn same_session(engine: &Engine, expected: &Session) -> Result<()> {
    require(
        crate::bridge::attachment_active(),
        "The overlay disconnected; the pending operation was cancelled.",
    )?;
    let current = engine.session()?;
    require(
        current.manager == expected.manager
            && current.world == expected.world
            && current.pawn == expected.pawn,
        "The map or active hero changed during the request. Inspect the item again.",
    )
}

fn object_field(engine: &Engine, object: u32, path: &str, kind: &str, target: &str) -> Result<u32> {
    let owner = path.rsplit_once('.').ok_or("Invalid property path.")?.0;
    engine.is_a(object, owner)?;
    let field = engine.reflect_field(path, kind, 4)?;
    require(
        field.count == 1 && u32_at(engine.reference(path)? + 0x84)? == engine.reference(target)?,
        "Pickup dependency field changed.",
    )?;
    let value = u32_at(object + field.offset)?;
    engine.is_a(value, target)?;
    Ok(value)
}

fn lifecycle_context(engine: &Engine, session: &Session, class: &str) -> Result<()> {
    let context = context_requirement(class)?;
    // Mana PostBeginPlay updates Main.NumberOfManaTokens. All supported world
    // pickups also use the normal Dungeon Defenders replication-info interface.
    let game = object_field(
        engine,
        session.world,
        "Engine.WorldInfo.Game",
        "Core.ObjectProperty",
        "Engine.GameInfo",
    )?;
    engine.is_a(game, "UDKGame.Main")?;
    let gri = object_field(
        engine,
        session.world,
        "Engine.WorldInfo.GRI",
        "Core.ObjectProperty",
        "Engine.GameReplicationInfo",
    )?;
    engine.is_a(gri, "UDKGame.DunDefGameReplicationInfo")?;
    match context {
        Context::Regular => Ok(()),
        Context::Golden => engine
            .is_a(gri, "DunDefSpecial.DunDefGRI_GoldenTokens")
            .map_err(|_| "Golden tokens require the golden-token mission controller; their collection callback is not valid in this map.".into()),
        Context::Delivery => {
            engine.is_a(gri, "DunDefSpecial.DunDefGRI_Delivery").map_err(|_| {
                "Presents require a delivery mission; physical spawning and collection outside that lifecycle have not been validated.".to_string()
            })?;
            let controller = object_field(
                engine, session.pawn, "Engine.Pawn.Controller", "Core.ObjectProperty", "Engine.Controller",
            )?;
            engine.is_a(controller, "DunDefSpecial.DunDefPlayerController_Delivery").map_err(|_| {
                "The active hero does not have the delivery controller required by present pickup/drop callbacks.".to_string()
            })?;
            validate_function(engine, INIT_MESH, 0x20102, 4, u16::MAX, INIT_PARAMETERS)?;
            Ok(())
        }
    }
}

fn validate_template(engine: &Engine, entry: &CatalogEntry, template: u32) -> Result<()> {
    require(
        engine.reference(&entry.path)? == template
            && u32_at(template + 0x34)? == engine.reference(&entry.class_name)?
            && u32_at(template + 8)? & 0x400 != 0,
        "Loaded object does not match the catalog class and archetype identity.",
    )?;
    engine.is_a(
        template,
        if entry.kind == ItemKind::WorldPickup {
            "Engine.Actor"
        } else {
            "UDKGame.HeroEquipment"
        },
    )
}

fn present_meshes(engine: &Engine, object: u32) -> Result<Vec<u32>> {
    engine.is_a(object, "DunDefSpecial.DunDefPickupableItem")?;
    let path = "DunDefSpecial.DunDefPickupableItem.myMeshes";
    let field = engine.reflect_field(path, "Core.ArrayProperty", 12)?;
    require(field.count == 1, "Present mesh array dimension changed.")?;
    let inner = u32_at(engine.reference(path)? + 0x84)?;
    require(
        u32_at(inner + 0x34)? == engine.reference("Core.ObjectProperty")?
            && u32_at(inner + 0x84)? == engine.reference("Engine.StaticMesh")?,
        "Present mesh-array element type changed.",
    )?;
    let meshes = array(object + field.offset, 64)?;
    require(!meshes.is_empty(), "Present template has no mesh variants.")?;
    for &mesh in &meshes {
        engine.is_a(mesh, "Engine.StaticMesh")?;
    }
    object_field(
        engine,
        object,
        "DunDefSpecial.DunDefPickupableItem.myActiveMesh",
        "Core.ComponentProperty",
        "Engine.StaticMeshComponent",
    )?;
    Ok(meshes)
}

pub(super) fn inspect(
    engine: &Engine,
    session: &Session,
    entry: &CatalogEntry,
    template: Option<u32>,
) -> Result<()> {
    require(
        entry.kind == ItemKind::WorldPickup,
        "Expected a world-pickup catalog entry.",
    )?;
    if let Some(reason) = &entry.unavailable_reason {
        return Err(reason.clone());
    }
    same_session(engine, session)?;
    lifecycle_context(engine, session, &entry.class_name)?;
    validate_function(engine, SPAWN, 0x24401, 52, 48, SPAWN_PARAMETERS)?;
    if let Some(template) = template {
        validate_template(engine, entry, template)?;
        if entry.class_name == "DunDefSpecial.DunDefPickupableItem" {
            present_meshes(engine, template)?;
        } else {
            let component = object_field(
                engine,
                template,
                "Engine.DynamicSMActor.StaticMeshComponent",
                "Core.ComponentProperty",
                "Engine.StaticMeshComponent",
            )?;
            object_field(
                engine,
                component,
                "Engine.StaticMeshComponent.StaticMesh",
                "Core.ObjectProperty",
                "Engine.StaticMesh",
            )?;
            if entry.class_name == "UDKGame.DunDefBuffToken" {
                object_field(
                    engine,
                    template,
                    "UDKGame.DunDefBuffToken.BuffTemplate",
                    "Core.ObjectProperty",
                    "UDKGame.DunDefBuff",
                )?;
            }
        }
    } else {
        validate_loader(engine)?;
    }
    Ok(())
}

fn verified_source(engine: &Engine, entry: &CatalogEntry) -> Result<PathBuf> {
    let snapshot = engine
        .catalog
        .as_ref()
        .ok_or("Bind the scanned catalog first.")?;
    let source = crate::catalog_runtime::preferred_source(snapshot, &entry.path)?;
    // The worker verified this source's content before scheduling this request.
    crate::catalog_runtime::source_path(std::path::Path::new(&snapshot.installation), &source.path)
}

#[cfg(target_arch = "x86")]
fn load_package(engine: &Engine, filename: &[u16]) -> Result<u32> {
    engine.check_thread()?;
    require(
        filename.last() == Some(&0),
        "Package filename is not terminated.",
    )?;
    // SAFETY: Engine::new pins the current executable. Current native caller and
    // callee analysis established this exact RVA and cdecl(Outer, UTF16, Flags).
    // The verified catalog filename stays alive for this synchronous call, and
    // the adapter rejects nested message-pump requests instead of blocking.
    let package = unsafe {
        let call: unsafe extern "C" fn(u32, *const u16, u32) -> u32 =
            std::mem::transmute((engine.base + LOAD_PACKAGE_RVA) as usize);
        // Current execDynamicLoadObject sets exactly 0x2002 for MayFail=true;
        // an independent shipped LoadPackage caller uses the same flag value.
        call(0, filename.as_ptr(), 0x2002)
    };
    engine.is_a(package, "Core.Package")?;
    Ok(package)
}

#[cfg(not(target_arch = "x86"))]
fn load_package(_: &Engine, _: &[u16]) -> Result<u32> {
    let _ = LOAD_PACKAGE_RVA;
    Err("Only the x86 adapter can load game packages.".into())
}

pub(super) fn load_template(
    engine: &mut Engine,
    session: &Session,
    entry: &CatalogEntry,
) -> Result<u32> {
    require(
        engine
            .catalog
            .as_ref()
            .is_some_and(|catalog| catalog.find(&entry.path).is_some()),
        "Only catalog paths may be loaded.",
    )?;
    if let Some(reason) = &entry.unavailable_reason {
        return Err(reason.clone());
    }
    dd_protocol::validate_grant(&entry.path, 1).map_err(str::to_string)?;
    same_session(engine, session)?;
    validate_loader(engine)?;
    if let Ok(template) = engine.reference(&entry.path) {
        validate_template(engine, entry, template)?;
        return Ok(template);
    }
    let package = verified_source(engine, entry)?;
    same_session(engine, session)?;
    // A loaded logical outer may already have the needed linker. Try the normal
    // reflected route before activating an entire cooked source package.
    let outer = entry
        .path
        .split('.')
        .next()
        .ok_or("Missing package identity.")?;
    if let Ok(package_object) = engine.reference(outer) {
        engine.is_a(package_object, "Core.Package")?;
        if let Some(template) = dynamic_load(engine, session, entry)? {
            return Ok(template);
        }
    }
    let filename: Vec<u16> = package.as_os_str().encode_wide().chain([0]).collect();
    let result = load_package(engine, &filename);
    engine.refresh_references()?;
    same_session(engine, session)?;
    result?;
    if let Ok(template) = engine.reference(&entry.path) {
        validate_template(engine, entry, template)?;
        return Ok(template);
    }
    dynamic_load(engine, session, entry)?.ok_or_else(|| {
        "The verified source package did not resolve this item path. Load its source map and inspect again.".into()
    })
}

fn dynamic_load(
    engine: &mut Engine,
    session: &Session,
    entry: &CatalogEntry,
) -> Result<Option<u32>> {
    validate_loader(engine)?;
    let expected = engine.reference(if entry.kind == ItemKind::WorldPickup {
        "Engine.Actor"
    } else {
        "UDKGame.HeroEquipment"
    })?;
    let name: Vec<u16> = entry.path.encode_utf16().chain([0]).collect();
    let mut parameters = [
        name.as_ptr() as usize as u32,
        name.len() as u32,
        name.len() as u32,
        expected,
        1,
        0,
    ];
    // The current native NativeParm/StrProperty trace establishes a deep copy
    // into the loader's temporary FString. ProcessEvent does not destroy input
    // parameter storage. Keep our UTF-16 vector alive through the checked call.
    engine.dispatch(session.manager, engine.reference(LOAD)?, &mut parameters)?;
    let loaded = parameters[5];
    engine.refresh_references()?;
    same_session(engine, session)?;
    if loaded == 0 {
        return Ok(None);
    }
    validate_template(engine, entry, loaded)?;
    Ok(Some(loaded))
}

fn spawn_location(location: [f32; 3], yaw: i32, index: u8) -> Result<[f32; 3]> {
    require(
        location
            .iter()
            .all(|v| v.is_finite() && v.abs() < 100_000_000.0),
        "Hero position is invalid.",
    )?;
    require(
        index < dd_protocol::MAX_QUANTITY,
        "Pickup batch index is invalid.",
    )?;
    let angle = (yaw as f64) * std::f64::consts::TAU / 65536.0;
    let (sin, cos) = angle.sin_cos();
    let side = if index == 0 {
        0.0
    } else {
        (index.div_ceil(2) as f64) * 48.0 * if index % 2 == 1 { 1.0 } else { -1.0 }
    };
    Ok([
        location[0] + (cos * 170.0 - sin * side) as f32,
        location[1] + (sin * 170.0 + cos * side) as f32,
        location[2] + 70.0,
    ])
}

fn spatial_field(engine: &Engine, actor: u32, path: &str, target: &str) -> Result<[u32; 3]> {
    engine.is_a(actor, "Engine.Actor")?;
    let field = engine.reflect_field(path, "Core.StructProperty", 12)?;
    require(
        field.count == 1 && u32_at(engine.reference(path)? + 0x84)? == engine.reference(target)?,
        "Actor spatial field changed.",
    )?;
    let data = read(actor + field.offset, 12)?;
    Ok([word(&data, 0), word(&data, 4), word(&data, 8)])
}

pub(super) fn spawn_one(
    engine: &Engine,
    session: &Session,
    template: u32,
    entry: &CatalogEntry,
    index: u8,
    uncertain: &mut bool,
) -> Result<()> {
    inspect(engine, session, entry, Some(template))?;
    let position = spatial_field(
        engine,
        session.pawn,
        "Engine.Actor.Location",
        "Core.Object.Vector",
    )?
    .map(f32::from_bits);
    let rotation = spatial_field(
        engine,
        session.pawn,
        "Engine.Actor.Rotation",
        "Core.Object.Rotator",
    )?;
    let location = spawn_location(position, rotation[1] as i32, index)?;
    let before = array(engine.base + super::OBJECTS, 2_000_000)?;
    let mut parameters = [0u32; 13];
    parameters[0] = engine.reference(&entry.class_name)?;
    parameters[1] = session.pawn;
    parameters[4..7].copy_from_slice(&location.map(f32::to_bits));
    parameters[7..10].copy_from_slice(&rotation);
    parameters[10] = template;
    // Name(None) and bNoCollisionFail=false preserve normal collision handling.
    *uncertain = true;
    engine.dispatch(session.pawn, engine.reference(SPAWN)?, &mut parameters)?;
    let actor = parameters[12];
    require(
        actor != 0 && actor != template && !before.contains(&actor),
        "The spawn did not return a fresh pickup actor; inspect the game before retrying.",
    )?;
    engine.is_a(actor, &entry.class_name)?;
    engine.actor_alive(actor)?;
    require(
        u32_at(actor + 0x34)? == engine.reference(&entry.class_name)?
            && u32_at(actor + 0x38)? == template
            && u32_at(actor + 8)? & 0x400 == 0
            && engine.value(actor, "Engine.Actor.WorldInfo")? == session.world,
        "Returned pickup class, archetype, or world differs from the request.",
    )?;
    if entry.class_name == "DunDefSpecial.DunDefPickupableItem" {
        let meshes = present_meshes(engine, actor)?;
        let function = validate_function(engine, INIT_MESH, 0x20102, 4, u16::MAX, INIT_PARAMETERS)?;
        engine.dispatch(actor, function, &mut [0])?;
        let index = engine.reflect_field(
            "DunDefSpecial.DunDefPickupableItem.myMeshIndex",
            "Core.IntProperty",
            4,
        )?;
        require(index.count == 1, "Present mesh-index dimension changed.")?;
        let selected = u32_at(actor + index.offset)? as usize;
        let component = object_field(
            engine,
            actor,
            "DunDefSpecial.DunDefPickupableItem.myActiveMesh",
            "Core.ComponentProperty",
            "Engine.StaticMeshComponent",
        )?;
        let mesh = object_field(
            engine,
            component,
            "Engine.StaticMeshComponent.StaticMesh",
            "Core.ObjectProperty",
            "Engine.StaticMesh",
        )?;
        require(
            meshes.get(selected) == Some(&mesh),
            "Present mesh initialization could not be confirmed.",
        )?;
    }
    same_session(engine, session)?;
    engine.actor_alive(actor)?;
    *uncertain = false;
    Ok(())
}

const LOAD_PARAMETERS: &[Parameter] = &[
    Parameter {
        name: "ObjectName",
        kind: "Core.StrProperty",
        offset: 0,
        size: 12,
        flags: 0x400080,
        target: None,
        meta_class: None,
    },
    Parameter {
        name: "ObjectClass",
        kind: "Core.ClassProperty",
        offset: 12,
        size: 4,
        flags: 0x80,
        target: Some("Core.Class"),
        meta_class: Some("Core.Object"),
    },
    Parameter {
        name: "MayFail",
        kind: "Core.BoolProperty",
        offset: 16,
        size: 4,
        flags: 0x90,
        target: None,
        meta_class: None,
    },
    Parameter {
        name: "ReturnValue",
        kind: "Core.ObjectProperty",
        offset: 20,
        size: 4,
        flags: 0x580,
        target: Some("Core.Object"),
        meta_class: None,
    },
];
const SPAWN_PARAMETERS: &[Parameter] = &[
    Parameter {
        name: "SpawnClass",
        kind: "Core.ClassProperty",
        offset: 0,
        size: 4,
        flags: 0x80,
        target: Some("Core.Class"),
        meta_class: Some("Engine.Actor"),
    },
    Parameter {
        name: "SpawnOwner",
        kind: "Core.ObjectProperty",
        offset: 4,
        size: 4,
        flags: 0x90,
        target: Some("Engine.Actor"),
        meta_class: None,
    },
    Parameter {
        name: "SpawnTag",
        kind: "Core.NameProperty",
        offset: 8,
        size: 8,
        flags: 0x90,
        target: None,
        meta_class: None,
    },
    Parameter {
        name: "SpawnLocation",
        kind: "Core.StructProperty",
        offset: 16,
        size: 12,
        flags: 0x90,
        target: Some("Core.Object.Vector"),
        meta_class: None,
    },
    Parameter {
        name: "SpawnRotation",
        kind: "Core.StructProperty",
        offset: 28,
        size: 12,
        flags: 0x90,
        target: Some("Core.Object.Rotator"),
        meta_class: None,
    },
    Parameter {
        name: "ActorTemplate",
        kind: "Core.ObjectProperty",
        offset: 40,
        size: 4,
        flags: 0x90,
        target: Some("Engine.Actor"),
        meta_class: None,
    },
    Parameter {
        name: "bNoCollisionFail",
        kind: "Core.BoolProperty",
        offset: 44,
        size: 4,
        flags: 0x90,
        target: None,
        meta_class: None,
    },
    Parameter {
        name: "ReturnValue",
        kind: "Core.ObjectProperty",
        offset: 48,
        size: 4,
        flags: 0xd80,
        target: Some("Engine.Actor"),
        meta_class: None,
    },
];
const INIT_PARAMETERS: &[Parameter] = &[Parameter {
    name: "MySpawner",
    kind: "Core.ObjectProperty",
    offset: 0,
    size: 4,
    flags: 0x80,
    target: Some("DunDefSpecial.DunDefPickupableSpawner"),
    meta_class: None,
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mission_pickups_cannot_enter_the_regular_lifecycle() {
        assert_eq!(
            context_requirement("UDKGame.DunDefManaToken").unwrap(),
            Context::Regular
        );
        assert_eq!(
            context_requirement("UDKGame.DunDefBuffToken").unwrap(),
            Context::Regular
        );
        assert_eq!(
            context_requirement("DunDefSpecial.DunDefManaToken_Golden").unwrap(),
            Context::Golden
        );
        assert_eq!(
            context_requirement("DunDefSpecial.DunDefPickupableItem").unwrap(),
            Context::Delivery
        );
        assert!(context_requirement("DunDefSpecial.CTF_Flag").is_err());
        assert!(context_requirement("UDKGame.HeroEquipment").is_err());
    }

    #[test]
    fn pickup_positions_follow_the_hero_and_reject_invalid_input() {
        assert_eq!(spawn_location([0.0; 3], 0, 0).unwrap(), [170.0, 0.0, 70.0]);
        let north = spawn_location([10.0, 20.0, 30.0], 16384, 0).unwrap();
        assert!((north[0] - 10.0).abs() < 0.001);
        assert!((north[1] - 190.0).abs() < 0.001);
        assert_eq!(north[2], 100.0);
        let positions: Vec<_> = (0..10)
            .map(|index| spawn_location([0.0; 3], 0, index).unwrap())
            .collect();
        for (i, position) in positions.iter().enumerate() {
            assert!(!positions[..i].contains(position));
        }
        assert!(spawn_location([f32::NAN, 0.0, 0.0], 0, 0).is_err());
        assert!(spawn_location([0.0; 3], 0, 10).is_err());
    }

    #[test]
    fn package_choice_prefers_english_startup_then_nonlocalized_sources() {
        let mut packages = [
            "UDKGame/CookedPCConsole/Startup_FRA.upk",
            "UDKGame/CookedPCConsole/map.upk",
            "UDKGame/CookedPCConsole/Startup_INT.upk",
        ];
        packages.sort_by_key(|p| crate::catalog_runtime::source_priority(p));
        assert!(packages[0].ends_with("Startup_INT.upk"));
        assert!(packages[1].ends_with("map.upk"));
    }
}
