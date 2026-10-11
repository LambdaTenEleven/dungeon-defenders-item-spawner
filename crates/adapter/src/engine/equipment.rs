//! Construct custom equipment before spawning, so the game initializes quality bookkeeping once.
use super::{Engine, Result, read, require, u32_at, word};
use dd_protocol::{CustomItem, WeaponStats};

const NET: &str = "UDKGame.HeroEquipment.EquipmentNetInfo";
const NATIVE: &str = "UDKGame.HeroEquipmentNative";
pub(super) const CREATE: &str = "UDKGame.HeroEquipment.CreateEquipmentFromNetInfo";
const RESISTANCES: [&str; 4] = ["Generic", "Fire", "Lightning", "Poison"];

pub(super) struct Property {
    pub path: &'static str,
    pub kind: &'static str,
    pub offset: u32,
    pub size: u32,
    pub count: u32,
    pub mask: u32,
    pub flags: u32,
    pub target: Option<&'static str>,
}

pub(super) fn validate(engine: &mut Engine) -> Result<()> {
    for property in super::layout::PROPERTIES {
        let field = engine.reflect_field(property.path, property.kind, property.size)?;
        require(
            field.offset == property.offset && field.count == property.count,
            &format!("Equipment layout changed: {}", property.path),
        )?;
        if property.mask != 0 {
            require(
                field.mask == property.mask,
                "Equipment boolean mask changed.",
            )?;
        }
        let address = engine.reference(property.path)?;
        if property.flags != 0 {
            require(
                u32_at(address + 0x4c)? == property.flags,
                "Constructor parameter flags changed.",
            )?;
        }
        if let Some(target) = property.target {
            require(
                u32_at(address + 0x84)? == engine.reference(target)?,
                "Equipment field target changed.",
            )?;
        }
        engine.fields.insert(property.path.into(), field);
    }
    for (path, kind) in [
        (
            "UDKGame.HeroEquipment.MinEquipmentLevels",
            "Core.IntProperty",
        ),
        ("UDKGame.HeroEquipment.MinDamageBonus", "Core.IntProperty"),
        (
            "UDKGame.HeroEquipment.EquipLevelRequirements",
            "Core.ArrayProperty",
        ),
    ] {
        let size = if kind == "Core.ArrayProperty" { 12 } else { 4 };
        engine
            .fields
            .insert(path.into(), engine.reflect_field(path, kind, size)?);
    }
    require(
        u32_at(engine.reference(NATIVE)? + 0x50)? == 540
            && u32_at(engine.reference(NET)? + 0x50)? == 412,
        "Equipment structure size changed.",
    )?;
    validate_constructor(engine)
}

fn validate_constructor(engine: &Engine) -> Result<()> {
    let function = engine.reference(CREATE)?;
    let header = read(function + 0x90, 0x18)?;
    require(
        u32_at(function + 0x34)? == engine.reference("Core.Function")?
            && word(&header, 0) == 0x426002
            && u16::from_le_bytes([header[4], header[5]]) == 0
            && u16::from_le_bytes([header[0x12], header[0x13]]) == 428
            && u16::from_le_bytes([header[0x14], header[0x15]]) == 424,
        "Equipment constructor ABI changed.",
    )
}

fn net_offset(engine: &Engine, name: &str) -> usize {
    engine.fields[&format!("{NET}.{name}")].offset as usize
}
fn put(data: &mut [u8], offset: usize, value: i32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn source_name(name: &str) -> &str {
    match name {
        "Description" => "EquipmentDescription",
        "bDisableRandomization" => "bDisableTheRandomization",
        _ => name,
    }
}

fn signed_byte(value: i32) -> u8 {
    value.wrapping_add(127) as u8
}

fn net_info(engine: &Engine, template: u32) -> Result<[u8; 412]> {
    let source = read(template, 540)?;
    let mut data = [0u8; 412];
    for property in super::layout::PROPERTIES
        .iter()
        .filter(|p| p.path.starts_with(NET))
    {
        let name = property.path.rsplit('.').next().expect("property leaf");
        let offset = property.offset as usize;
        match name {
            "EquipmentTemplate" => {
                put(&mut data, offset, template as i32);
                continue;
            }
            "EquipmentID1" | "EquipmentID2" | "bIsSecondary" => continue,
            "FolderID" => {
                put(&mut data, offset, -1);
                continue;
            }
            _ => {}
        }
        let source_path = format!("{NATIVE}.{}", source_name(name));
        let field = engine.fields[&source_path];
        let start = field.offset as usize;
        if name == "DamageReductions" {
            for index in 0..4 {
                data[index * 8..index * 8 + 4]
                    .copy_from_slice(&source[start + index * 8..start + index * 8 + 4]);
                data[index * 8 + 4] = signed_byte(word(&source, start + index * 8 + 4) as i32);
            }
        } else if name == "StatModifiers" {
            for index in 0..11 {
                put(
                    &mut data,
                    offset + index * 4,
                    (word(&source, start + index * 4) as i32).wrapping_add(127),
                );
            }
        } else if property.kind == "Core.ByteProperty" && field.size == 4 {
            let value = word(&source, start);
            data[offset] = if matches!(name, "bCanBeUpgraded" | "AllowRenamingAtMaxUpgrade") {
                u8::from(value & field.mask != 0)
            } else {
                signed_byte(value as i32)
            };
        } else if property.kind == "Core.BoolProperty" {
            if word(&source, start) & field.mask != 0 {
                let value = word(&data, offset) | property.mask;
                put(&mut data, offset, value as i32);
            }
        } else {
            let length = (property.size * property.count) as usize;
            if property.kind == "Core.StrProperty" {
                // Borrow template-owned FString storage only for the const-out constructor call.
                let count = word(&source, start + 4);
                let capacity = word(&source, start + 8);
                require(
                    count <= capacity && capacity <= 65_536,
                    "Invalid template string.",
                )?;
                if count > 0 {
                    read(word(&source, start), count as usize * 2)?;
                }
            }
            data[offset..offset + length].copy_from_slice(&source[start..start + length]);
        }
    }
    Ok(data)
}

fn weapon_values(stats: &WeaponStats) -> [(&str, Option<i32>); 10] {
    [
        ("WeaponDamageBonus", stats.damage),
        ("WeaponAdditionalDamageAmount", stats.elemental_damage),
        ("WeaponNumberOfProjectilesBonus", stats.projectiles),
        ("WeaponSpeedOfProjectilesBonus", stats.projectile_speed),
        ("WeaponShotsPerSecondBonus", stats.attack_rate),
        ("WeaponClipAmmoBonus", stats.clip_ammo),
        ("WeaponReloadSpeedBonus", stats.reload),
        ("WeaponKnockbackBonus", stats.knockback),
        ("WeaponChargeSpeedBonus", stats.charge_speed),
        ("WeaponBlockingBonus", stats.blocking),
    ]
}

fn overrides(
    engine: &Engine,
    template: u32,
    kind: u32,
    settings: &CustomItem,
    data: &mut [u8; 412],
) -> Result<()> {
    settings.validate().map_err(str::to_string)?;
    require(
        kind == 0 || settings.weapon == WeaponStats::default(),
        "Weapon bonuses cannot be applied to armor.",
    )?;
    require(
        kind != 0 || settings.resistances == [None; 4],
        "Armor resistances cannot be applied to a weapon.",
    )?;
    if let Some(quality) = settings.quality {
        let tiers = super::array(
            engine.field_address(template, "UDKGame.HeroEquipment.EquipLevelRequirements")?,
            64,
        )?;
        require(
            (quality as usize) < tiers.len(),
            "This template does not support the selected quality tier.",
        )?;
        data[net_offset(engine, "NameIndex_QualityDescriptor")] = quality as u8;
    }
    let stats_offset = net_offset(engine, "StatModifiers");
    for (index, value) in settings.hero_stats.iter().enumerate() {
        if let Some(value) = value {
            put(data, stats_offset + (index + 1) * 4, value + 127);
        }
    }
    for (name, value) in [
        ("Level", settings.upgrade_level),
        ("MaxEquipmentLevel", settings.max_upgrades),
    ] {
        if let Some(value) = value {
            put(data, net_offset(engine, name), value);
        }
    }
    let level = word(data, net_offset(engine, "Level")) as i32;
    let max = word(data, net_offset(engine, "MaxEquipmentLevel")) as i32;
    let minimum = engine.value(template, "UDKGame.HeroEquipment.MinEquipmentLevels")? as i32;
    if settings.upgrade_level.is_some() || settings.max_upgrades.is_some() {
        require(
            level.max(1) <= max.max(minimum),
            "Current upgrade level exceeds this item's maximum; set Maximum upgrades too.",
        )?;
    }
    if let Some(max) = settings.max_upgrades {
        require(
            max >= minimum,
            &format!("This template requires at least {minimum} maximum upgrades."),
        )?;
    }
    if let Some(damage) = settings.weapon.damage {
        let minimum = engine.value(template, "UDKGame.HeroEquipment.MinDamageBonus")? as i32;
        require(
            damage >= minimum,
            &format!("This weapon requires at least {minimum} damage bonus."),
        )?;
    }
    for (name, value) in weapon_values(&settings.weapon) {
        if let Some(value) = value {
            let field = engine.fields[&format!("{NET}.{name}")];
            if field.size == 1 {
                data[field.offset as usize] = signed_byte(value);
            } else {
                put(data, field.offset as usize, value);
            }
        }
    }
    for (name, value) in RESISTANCES.iter().zip(settings.resistances) {
        if let Some(value) = value {
            let class = engine.reference(&format!("UDKGame.DunDefDamageType_{name}"))?;
            let matching: Vec<_> = (0..4).filter(|i| word(data, i * 8) == class).collect();
            require(
                matching.len() == 1,
                "Template resistance types are unsupported.",
            )?;
            data[matching[0] * 8 + 4] = signed_byte(value);
        }
    }
    Ok(())
}

pub(super) fn create(
    engine: &Engine,
    template: u32,
    owner: u32,
    kind: u32,
    settings: &CustomItem,
) -> Result<u32> {
    validate_constructor(engine)?;
    let mut data = net_info(engine, template)?;
    overrides(engine, template, kind, settings, &mut data)?;
    let mut params = [0u32; 107];
    for (slot, bytes) in params.iter_mut().zip(data.as_chunks::<4>().0) {
        *slot = u32::from_le_bytes(*bytes);
    }
    params[104] = owner;
    // The reflected const-out struct is caller owned. The dispatcher skips destroying out params;
    // InitFromNetInfo copies strings into the new object. No engine allocation is owned by Rust.
    // Creation and spawning are consecutive game-thread calls; this temporary never crosses a tick.
    engine.dispatch(template, engine.reference(CREATE)?, &mut params)?;
    let item = params[106];
    engine.is_a(item, "UDKGame.HeroEquipment")?;
    require(
        item != template && u32_at(item + 0x38)? == template && u32_at(item + 8)? & 0x400 == 0,
        "Constructor did not return a fresh equipment instance.",
    )?;
    verify(engine, item, settings)?;
    Ok(item)
}

pub(super) fn verify(engine: &Engine, item: u32, settings: &CustomItem) -> Result<()> {
    let source = read(item, 540)?;
    let value = |name: &str, index: usize| {
        let field = engine.fields[&format!("{NATIVE}.{name}")];
        let offset = field.offset as usize + index * field.size as usize;
        if field.size == 1 {
            source[offset] as i32
        } else {
            word(&source, offset) as i32
        }
    };
    if let Some(quality) = settings.quality {
        require(
            value("NameIndex_QualityDescriptor", 0) == quality as i32,
            "Quality readback differs from the request.",
        )?;
    }
    for (index, expected) in settings.hero_stats.iter().enumerate() {
        if let Some(expected) = expected {
            require(
                value("StatModifiers", index + 1) == *expected,
                "Hero stat readback differs from the request.",
            )?;
        }
    }
    for (name, expected) in weapon_values(&settings.weapon).into_iter().chain([
        ("Level", settings.upgrade_level),
        ("MaxEquipmentLevel", settings.max_upgrades),
    ]) {
        if let Some(expected) = expected {
            require(
                value(name, 0) == expected,
                &format!("{name} readback differs from the request."),
            )?;
        }
    }
    let offset = engine.fields[&format!("{NATIVE}.DamageReductions")].offset as usize;
    for (name, expected) in RESISTANCES.iter().zip(settings.resistances) {
        if let Some(expected) = expected {
            let class = engine.reference(&format!("UDKGame.DunDefDamageType_{name}"))?;
            require(
                (0..4).any(|i| {
                    word(&source, offset + i * 8) == class
                        && word(&source, offset + i * 8 + 4) as i32 == expected
                }),
                "Resistance readback differs from the request.",
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_byte_boundaries_survive_the_game_encoding() {
        for value in [-127, -1, 0, 1, 128] {
            assert_eq!(i32::from(signed_byte(value)) - 127, value);
        }
    }
}
