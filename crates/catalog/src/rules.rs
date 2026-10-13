//! Preset behavior rules for discovered objects, never an item-name allowlist.
//!
//! A recognized category is descriptive. Admission additionally requires an
//! exact reviewed class, complete inheritance and unambiguous package facts.
use crate::{
    CatalogEntry, ExportRecord, ItemCapabilities, ItemKind, PackageFacts, PetCapabilities,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

type Properties = BTreeMap<String, Value>;
type Graph = BTreeMap<String, Option<String>>;

const EQUIPMENT: &str = "UDKGame.HeroEquipment";
const NATIVE_EQUIPMENT: &str = "UDKGame.HeroEquipmentNative";
const FAMILIAR: &str = "UDKGame.HeroEquipment_Familiar";
const PET_CLASSES: &[(&str, &str)] = &[
    (
        "UDKGame.HeroEquipment_Familiar_WithProjectileAI",
        "Projectile",
    ),
    ("UDKGame.HeroEquipment_Familiar_Melee", "Melee"),
    (
        "UDKGame.HeroEquipment_Familiar_PlayerHealer",
        "Player healing",
    ),
    (
        "UDKGame.HeroEquipment_Familiar_TowerHealer",
        "Tower healing",
    ),
    ("UDKGame.HeroEquipment_Familiar_TowerBooster", "Guardian"),
    (
        "UDKGame.HeroEquipment_Familiar_MoneyGiver",
        "Mana generation",
    ),
    (
        "UDKGame.HeroEquipment_Familiar_PawnBooster",
        "Hero boosting",
    ),
    (
        "UDKGame.HeroEquipment_Familiar_Buff_Spawner",
        "Buff spawning",
    ),
    ("UDKGame.HeroEquipment_Familiar_TADPS", "Target damage"),
    ("CDT.HeroEquipment_Familiar_SheepPet", "Sheep"),
    ("CDT4.HeroEquipment_Familiar_CoreHealer", "Crystal healing"),
    ("CDT4.HeroEquipment_Familiar_MiniQueen", "Mini queen"),
    (
        "CDT7.HeroEquipment_Familiar_Melee_TowerScaling",
        "Tower-scaled melee",
    ),
    (
        "CDT7.HeroEquipment_Familiar_TowerDamageScaling",
        "Tower-scaled damage",
    ),
    ("CDT8.HeroEquipment_Familiar_AoeBuffer", "Area boosting"),
    ("CDT8.HeroEquipment_Familiar_SummonAllies", "Ally summoning"),
];
const WORLD_CLASSES: &[(&str, &str)] = &[
    ("UDKGame.DunDefManaToken", "Loose mana"),
    ("UDKGame.DunDefBuffToken", "Fruit buffs"),
    (
        "DunDefSpecial.DunDefManaToken_Golden",
        "Golden mission tokens",
    ),
    ("DunDefSpecial.DunDefPickupableItem", "Presents"),
    ("DunDefSpecial.CTF_Flag", "Flags"),
];

fn descendant(class: &str, parent: &str, graph: &Graph) -> bool {
    let mut cursor = Some(class);
    let mut seen = BTreeSet::new();
    while let Some(current) = cursor {
        if current == parent {
            return true;
        }
        if !seen.insert(current) || seen.len() > 128 {
            return false;
        }
        cursor = graph.get(current).and_then(Option::as_deref);
    }
    false
}

/// Only equipment and known pickup hierarchies are candidates. The name fallback
/// preserves incompletely linked equipment exports for unavailable diagnostics;
/// it never grants support to an unfamiliar class.
pub fn is_candidate(class: &str, graph: &Graph) -> bool {
    descendant(class, NATIVE_EQUIPMENT, graph)
        || descendant(class, EQUIPMENT, graph)
        || class
            .rsplit('.')
            .next()
            .is_some_and(|name| name == "HeroEquipment" || name.starts_with("HeroEquipment_"))
        || WORLD_CLASSES
            .iter()
            .any(|(root, _)| descendant(class, root, graph))
}

pub fn reviewed_class(class: &str) -> bool {
    class == EQUIPMENT
        || class == "UDKGame.HeroEquipment_EventHostCrown"
        || PET_CLASSES.iter().any(|(name, _)| *name == class)
        || WORLD_CLASSES.iter().any(|(name, _)| *name == class)
}

/// Runtime admission uses the actual class and reflected fields, not a cached
/// category. Unknown subclasses cannot inherit a reviewed parent's permission.
pub fn category_from_runtime(class: &str, equipment_type: u8, can_equip: bool) -> ItemKind {
    if WORLD_CLASSES.iter().any(|(name, _)| *name == class) {
        return ItemKind::WorldPickup;
    }
    if PET_CLASSES.iter().any(|(name, _)| *name == class) {
        return ItemKind::Pet;
    }
    if !matches!(class, EQUIPMENT | "UDKGame.HeroEquipment_EventHostCrown") {
        return ItemKind::Unknown;
    }
    if class == "UDKGame.HeroEquipment_EventHostCrown" && equipment_type != 7 {
        return ItemKind::Unknown;
    }
    if !can_equip {
        return ItemKind::Collectible;
    }
    match equipment_type {
        0 => ItemKind::Weapon,
        1..=4 => ItemKind::Armor,
        7..=10 => ItemKind::Accessory,
        _ => ItemKind::Unknown,
    }
}

/// Upper bounds for independently validating an untrusted cache. Property-based
/// restrictions must still be applied with [`capabilities`] before admission.
pub fn maximum_capabilities(kind: ItemKind, class: &str) -> ItemCapabilities {
    let mut result = ItemCapabilities::default();
    let pet_class = PET_CLASSES.iter().any(|(name, _)| *name == class);
    let admitted = match kind {
        ItemKind::Weapon | ItemKind::Armor | ItemKind::Collectible => class == EQUIPMENT,
        ItemKind::Accessory => matches!(class, EQUIPMENT | "UDKGame.HeroEquipment_EventHostCrown"),
        ItemKind::Pet => pet_class,
        ItemKind::WorldPickup => WORLD_CLASSES
            .iter()
            .any(|(name, group)| *name == class && *group != "Flags"),
        ItemKind::Unknown => false,
    };
    if !admitted {
        return result;
    }
    result.template_defaults = true;
    result.random_roll = matches!(
        kind,
        ItemKind::Weapon | ItemKind::Armor | ItemKind::Pet | ItemKind::Accessory
    );
    result.custom_common = result.random_roll;
    result.resistances = matches!(kind, ItemKind::Armor | ItemKind::Accessory);
    result.weapon = kind == ItemKind::Weapon;
    result.stored_mana = kind == ItemKind::Collectible;
    if kind == ItemKind::Pet {
        let mut pet = PetCapabilities::default();
        match class {
            "UDKGame.HeroEquipment_Familiar_WithProjectileAI"
            | "UDKGame.HeroEquipment_Familiar_Melee" => {
                pet.damage = true;
                pet.attack_rate = true;
                pet.projectiles = true;
                pet.projectile_speed = true;
            }
            "UDKGame.HeroEquipment_Familiar_PlayerHealer"
            | "UDKGame.HeroEquipment_Familiar_TowerHealer" => {
                pet.healing = true;
                pet.attack_rate = true;
            }
            "UDKGame.HeroEquipment_Familiar_MoneyGiver" => pet.mana = true,
            "UDKGame.HeroEquipment_Familiar_TowerBooster" => {
                pet.boost = true;
                pet.range = true;
            }
            _ => {}
        }
        result.pet = Some(pet);
    }
    result
}

fn flag(props: &Properties, name: &str) -> bool {
    props.get(name).and_then(Value::as_bool).unwrap_or(false)
}

/// Refine known field meanings using resolved defaults or actual runtime values.
pub fn capabilities(kind: ItemKind, class: &str, props: &Properties) -> ItemCapabilities {
    if [
        "bCantSave",
        "bCantPutInItemBox",
        "bIsConsumable",
        "bForceUseParentTemplate",
    ]
    .iter()
    .any(|name| flag(props, name))
    {
        return ItemCapabilities::default();
    }
    let mut result = maximum_capabilities(kind, class);
    result.random_roll &= !flag(props, "bDisableTheRandomization");
    // The high-digit flag alone is also used by radioactive coal. The shop
    // denomination flag identifies the reviewed stored-value serialization route.
    result.stored_mana &= flag(props, "bUseHighDigitManaTokenValue")
        && flag(props, "bPlayerShopForceToMinimumSellWorth");
    if let Some(pet) = result.pet.as_mut()
        && matches!(
            class,
            "UDKGame.HeroEquipment_Familiar_WithProjectileAI"
                | "UDKGame.HeroEquipment_Familiar_Melee"
        )
    {
        pet.damage = flag(props, "UseWeaponCoreStats");
        pet.attack_rate = pet.damage && !flag(props, "bUseFixedShootSpeed");
        pet.projectiles =
            class.ends_with("WithProjectileAI") || flag(props, "bAlsoShootProjectile");
        pet.projectile_speed = pet.projectiles;
    }
    result
}

#[derive(Clone, Copy)]
struct Record<'a> {
    package: &'a str,
    index: usize,
    export: &'a ExportRecord,
}

fn preferred<'a>(rows: &[Record<'a>], package: &str) -> Record<'a> {
    *rows
        .iter()
        .min_by_key(|row| {
            (
                row.package != package,
                !row.package.ends_with("Startup_INT.upk"),
                !row.package.ends_with("UDKGame.upk"),
                row.package,
            )
        })
        .expect("record groups are nonempty")
}

struct Resolver<'a> {
    graph: Graph,
    conflicts: BTreeSet<String>,
    records: BTreeMap<String, Vec<Record<'a>>>,
    defaults: BTreeMap<String, Vec<Record<'a>>>,
    resolved: RefCell<BTreeMap<(String, usize), Result<Properties, String>>>,
}

impl Resolver<'_> {
    fn resolve(
        &self,
        row: Record<'_>,
        seen: &mut BTreeSet<(String, usize)>,
    ) -> Result<Properties, String> {
        let key = (row.package.to_owned(), row.index);
        if let Some(cached) = self.resolved.borrow().get(&key) {
            return cached.clone();
        }
        if seen.len() >= 128 || !seen.insert(key.clone()) {
            return Err(
                "Class-default or archetype inheritance is cyclic or exceeds the supported depth."
                    .into(),
            );
        }
        let result = self.resolve_inner(row, seen);
        seen.remove(&key);
        self.resolved.borrow_mut().insert(key, result.clone());
        result
    }

    fn resolve_group(
        &self,
        rows: &[Record<'_>],
        package: &str,
        seen: &mut BTreeSet<(String, usize)>,
    ) -> Result<Properties, String> {
        let chosen = preferred(rows, package);
        let properties = self.resolve(chosen, seen)?;
        for row in rows {
            if row.package == chosen.package && row.index == chosen.index {
                continue;
            }
            if let Some(difference) = definition_difference(
                chosen,
                &properties,
                *row,
                &self.resolve(*row, seen)?,
                &self.graph,
            ) {
                return Err(format!(
                    "Conflicting package definitions for inherited object {}: {difference} differs.",
                    chosen.export.path,
                ));
            }
        }
        Ok(properties)
    }

    fn resolve_inner(
        &self,
        row: Record<'_>,
        seen: &mut BTreeSet<(String, usize)>,
    ) -> Result<Properties, String> {
        let record = row.export;
        if let Some(error) = &record.property_error {
            return Err(format!("Properties could not be decoded: {error}"));
        }
        if self.conflicts.contains(&record.class_name) {
            return Err(format!(
                "Conflicting class definitions for {}.",
                record.class_name
            ));
        }
        let mut props = Properties::new();
        let base = if record.is_class_default {
            self.graph
                .get(&record.class_name)
                .and_then(Option::as_deref)
        } else {
            Some(record.class_name.as_str())
        };
        if let Some(base) = base
            && base != NATIVE_EQUIPMENT
            && is_candidate(base, &self.graph)
        {
            let defaults = self.defaults.get(base).ok_or_else(|| {
                format!("Class defaults for {base} were not found in the scanned packages.")
            })?;
            props.extend(self.resolve_group(defaults, row.package, seen)?);
        }
        if let Some(parent) = record.archetype.as_deref() {
            if let Some(parents) = self.records.get(&parent.to_ascii_lowercase()) {
                props.extend(self.resolve_group(parents, row.package, seen)?);
            } else if parent != "UDKGame.Default__HeroEquipmentNative"
                && !(record.is_class_default
                    && base.is_some_and(|class| !is_candidate(class, &self.graph)))
            {
                return Err(format!(
                    "Archetype {parent} was not found in the scanned packages."
                ));
            }
        }
        props.extend(record.properties.clone());
        Ok(props)
    }
}

fn equipment_type(props: &Properties) -> Option<u8> {
    let Some(value) = props.get("EquipmentType") else {
        return Some(0);
    };
    if let Some(number) = value.as_u64() {
        return u8::try_from(number).ok();
    }
    match value.as_str()? {
        "EQT_WEAPON" => Some(0),
        "EQT_ARMOR_TORSO" => Some(1),
        "EQT_ARMOR_PANTS" => Some(2),
        "EQT_ARMOR_BOOTS" => Some(3),
        "EQT_ARMOR_GLOVES" => Some(4),
        "EQT_FAMILIAR" => Some(5),
        "EQT_ENDPRIMARYITEMS" => Some(6),
        "EQT_ACCESSORY1" => Some(7),
        "EQT_ACCESSORY2" => Some(8),
        "EQT_ACCESSORY3" => Some(9),
        "EQT_MASK" => Some(10),
        _ => None,
    }
}

/// Reviewed family refinements require an independently verified unequippable
/// equipment object. A matching name is never sufficient runtime admission.
pub fn collectible_group(path: &str, props: &Properties) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    if capabilities(ItemKind::Collectible, EQUIPMENT, props).stored_mana
        || lower.contains(".manatoken.") && lower.contains("mana_equipment")
    {
        Some("Inventory mana tokens")
    } else if lower.contains(".bossrush.exp_equipment") {
        Some("Experience Orbs")
    } else if lower.starts_with("eastereggs.") || lower.contains("organiceasteregg") {
        Some("Eggs")
    } else if ["coal_", "heartfragment_", "embertoken_"]
        .iter()
        .any(|token| lower.contains(token))
    {
        Some("Resources")
    } else if ["hornpack", "_shard_equipment", "zamirasgift_equipment"]
        .iter()
        .any(|token| lower.contains(token))
    {
        Some("Exchange tokens")
    } else {
        None
    }
}

fn variants(kind: ItemKind, path: &str) -> Vec<String> {
    let leaf = path.rsplit('.').next().unwrap_or(path).to_ascii_lowercase();
    let tags: &[&str] = match kind {
        ItemKind::Weapon => &["LD", "Ruthless", "Boss", "EMH"],
        ItemKind::Armor => &["Leather", "Chain", "Mail", "Plate", "Pristine"],
        _ => &[],
    };
    let mut result: Vec<String> = tags
        .iter()
        .filter(|tag| {
            leaf.split('_').any(|token| {
                if **tag == "LD" {
                    token == "ld"
                } else {
                    token.starts_with(&tag.to_ascii_lowercase())
                }
            })
        })
        .map(|tag| (*tag).to_owned())
        .collect();
    if kind == ItemKind::Weapon && result.is_empty() {
        result.push("Base".into());
    }
    if leaf.contains("tigitest") {
        result.push("Test".into());
    }
    result
}

fn semantic_properties(props: &Properties, class: &str, graph: &Graph) -> Properties {
    let equipment =
        descendant(class, NATIVE_EQUIPMENT, graph) || descendant(class, EQUIPMENT, graph);
    props
        .iter()
        .filter(|(name, value)| {
            // Cooked copies can have different template lookup IDs despite identical
            // behavior. Ignore only a valid equipment IntProperty for comparison;
            // retain the original facts and the game's actual runtime template/ID.
            if equipment
                && name.as_str() == "equipmentTemplateUniqueID"
                && value.as_i64().is_some_and(|id| i32::try_from(id).is_ok())
            {
                return false;
            }
            !matches!(
                name.as_str(),
                "EquipmentName"
                    | "UserEquipmentName"
                    | "EquipmentDescription"
                    | "UserForgerName"
                    | "BaseForgerName"
                    | "LevelString"
                    | "ForgedByDescription"
                    | "AdditionalDescription"
                    | "DamageDescription"
                    | "GivenEquipmentMessageString"
                    | "RequiredLevelString"
                    | "RequiredClassString"
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn definition_difference(
    left: Record<'_>,
    left_props: &Properties,
    right: Record<'_>,
    right_props: &Properties,
    graph: &Graph,
) -> Option<String> {
    if left.export.class_name != right.export.class_name {
        return Some("class".into());
    }
    if left.export.is_archetype != right.export.is_archetype {
        return Some("archetype flag".into());
    }
    let archetype = |row: Record<'_>| {
        row.export
            .archetype
            .as_deref()
            .map(|path| path.to_ascii_lowercase())
    };
    if archetype(left) != archetype(right) {
        return Some("archetype reference".into());
    }
    let left = semantic_properties(left_props, &left.export.class_name, graph);
    let right = semantic_properties(right_props, &right.export.class_name, graph);
    left.keys()
        .chain(right.keys())
        .find(|key| left.get(*key) != right.get(*key))
        .cloned()
}

fn describe(row: Record<'_>, graph: &Graph, props: &Properties) -> CatalogEntry {
    let record = row.export;
    let world = WORLD_CLASSES
        .iter()
        .find(|(class, _)| *class == record.class_name)
        .or_else(|| {
            WORLD_CLASSES
                .iter()
                .find(|(class, _)| descendant(&record.class_name, class, graph))
        });
    let pet = descendant(&record.class_name, FAMILIAR, graph)
        || PET_CLASSES
            .iter()
            .any(|(class, _)| *class == record.class_name);
    let equipment_type = if world.is_some() {
        None
    } else {
        equipment_type(props)
    };
    let kind = if world.is_some() {
        ItemKind::WorldPickup
    } else if pet {
        ItemKind::Pet
    } else if props.get("bCanBeEquipped") == Some(&Value::Bool(false)) {
        ItemKind::Collectible
    } else {
        match equipment_type {
            Some(0) => ItemKind::Weapon,
            Some(1..=4) => ItemKind::Armor,
            Some(7..=10) => ItemKind::Accessory,
            _ => ItemKind::Unknown,
        }
    };
    let group = match kind {
        ItemKind::WorldPickup => world.map_or("Unknown pickups", |(_, group)| *group),
        ItemKind::Pet => PET_CLASSES
            .iter()
            .find(|(class, _)| *class == record.class_name)
            .map_or("Other pets", |(_, group)| *group),
        ItemKind::Collectible => {
            collectible_group(&record.path, props).unwrap_or("Unknown collectibles")
        }
        ItemKind::Weapon => match props.get("weaponType").and_then(Value::as_str) {
            Some("EWT_WEAPON_SQUIRE") => "Squire",
            Some("EWT_WEAPON_APPRENTICE") => "Apprentice",
            Some("EWT_WEAPON_INITIATE") => "Huntress",
            Some("EWT_WEAPON_RECRUIT") => "Monk",
            _ => "Other",
        },
        ItemKind::Armor if flag(props, "UsesEquipmentAttachments") => "Other",
        ItemKind::Armor => match equipment_type {
            Some(1) => "Torso",
            Some(2) => "Helmet",
            Some(3) => "Boots",
            Some(4) => "Gauntlets",
            _ => "Other",
        },
        ItemKind::Accessory => match equipment_type {
            Some(7) => "Hat/Brooch",
            Some(8) => "Bracers",
            Some(9) => "Shield",
            Some(10) => "Mask",
            _ => "Other",
        },
        ItemKind::Unknown => "Unclassified equipment",
    };
    let leaf = record.path.rsplit('.').next().unwrap_or(&record.path);
    let name = ["UserEquipmentName", "EquipmentName"]
        .iter()
        .find_map(|key| {
            props
                .get(*key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .map(str::to_owned)
        .unwrap_or_else(|| {
            leaf.trim_start_matches("HeroEquipment_")
                .trim_start_matches("Equipment_")
                .to_owned()
        });
    let name = match record.class_name.as_str() {
        "UDKGame.DunDefManaToken" => format!(
            "Mana gem ({})",
            props
                .get("ManaPower")
                .and_then(Value::as_f64)
                .unwrap_or(1.0)
        ),
        "DunDefSpecial.DunDefManaToken_Golden" => "Golden mission token".into(),
        "DunDefSpecial.DunDefPickupableItem" => "Holiday present".into(),
        "UDKGame.DunDefBuffToken" => props
            .get("BuffTemplate")
            .and_then(Value::as_str)
            .map_or_else(
                || leaf.to_owned(),
                |value| format!("Fruit: {}", value.rsplit('.').next().unwrap_or(value)),
            ),
        _ => name,
    };
    let forced_parent_reason = record.archetype.as_deref().map(|parent| {
        format!(
            "This template forces its parent; choose {} instead.",
            parent.to_owned()
        )
    });
    let reason = if !record.path.contains('.') {
        Some("Unqualified export identity; a unique runtime object path has not been verified.")
    } else if !record.is_archetype || record.path.starts_with("TheWorld.") {
        Some("Placed level instance, not a reusable pickup archetype.")
    } else if !reviewed_class(&record.class_name) {
        Some("Class behavior has not been reviewed; this discovered item remains unavailable.")
    } else if kind == ItemKind::Unknown {
        Some("Unrecognized equipment type; classification requires further verification.")
    } else if flag(props, "bCantSave") || flag(props, "bCantPutInItemBox") {
        Some("Template explicitly forbids saving or item-box storage.")
    } else if flag(props, "bIsConsumable") {
        Some("Consumable lifecycle is not supported by this integration.")
    } else if flag(props, "bForceUseParentTemplate") {
        Some(
            forced_parent_reason
                .as_deref()
                .unwrap_or("This template forces its parent; choose the parent template instead."),
        )
    } else if group == "Flags" {
        Some("Flag team/base initialization and actor lifecycle have not been verified.")
    } else if kind == ItemKind::Collectible
        && (group == "Unknown collectibles"
            || equipment_type != Some(5)
            || props
                .get("DroppedEquipmentTemplates")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty))
    {
        Some("Collectible family or physical-pickup initialization has not been verified.")
    } else {
        None
    };
    CatalogEntry {
        name,
        path: record.path.clone(),
        class_name: record.class_name.clone(),
        source_packages: vec![],
        kind,
        group: group.into(),
        variants: variants(kind, &record.path),
        equipment_type,
        capabilities: if reason.is_none() {
            capabilities(kind, &record.class_name, props)
        } else {
            ItemCapabilities::default()
        },
        unavailable_reason: reason.map(str::to_owned),
        classification_rule: format!(
            "v{}/{}/{}",
            crate::RULES_VERSION,
            match kind {
                ItemKind::Weapon => "weapon-type",
                ItemKind::Armor => "armor-slot",
                ItemKind::Pet => "familiar-class",
                ItemKind::Accessory => "accessory-slot",
                ItemKind::Collectible => "unequippable-family",
                ItemKind::WorldPickup => "pickup-class",
                ItemKind::Unknown => "unknown",
            },
            group
        ),
    }
}

/// Build a deterministic catalog from all scanned packages. Equivalent cooked
/// copies collapse by runtime identity; conflicting gameplay definitions block
/// that identity instead of selecting whichever file was encountered first.
pub fn classify(packages: &[PackageFacts]) -> Vec<CatalogEntry> {
    let mut resolver = Resolver {
        graph: Graph::new(),
        conflicts: BTreeSet::new(),
        records: BTreeMap::new(),
        defaults: BTreeMap::new(),
        resolved: RefCell::new(BTreeMap::new()),
    };
    for package in packages {
        for class in &package.classes {
            if resolver
                .graph
                .get(&class.path)
                .is_some_and(|parent| *parent != class.parent)
            {
                resolver.conflicts.insert(class.path.clone());
            } else {
                resolver
                    .graph
                    .insert(class.path.clone(), class.parent.clone());
            }
        }
        for (index, export) in package.exports.iter().enumerate() {
            let row = Record {
                package: &package.source.path,
                index,
                export,
            };
            resolver
                .records
                .entry(export.path.to_ascii_lowercase())
                .or_default()
                .push(row);
            if export.is_class_default {
                resolver
                    .defaults
                    .entry(export.class_name.clone())
                    .or_default()
                    .push(row);
            }
        }
    }
    let mut candidates: BTreeMap<String, Vec<Record<'_>>> = BTreeMap::new();
    for package in packages {
        for (index, export) in package.exports.iter().enumerate() {
            if !export.is_class_default
                && is_candidate(&export.class_name, &resolver.graph)
                && (export.is_archetype
                    || WORLD_CLASSES
                        .iter()
                        .any(|(class, _)| descendant(&export.class_name, class, &resolver.graph)))
            {
                candidates
                    .entry(export.path.to_ascii_lowercase())
                    .or_default()
                    .push(Record {
                        package: &package.source.path,
                        index,
                        export,
                    });
            }
        }
    }
    candidates
        .into_values()
        .map(|rows| {
            let preferred = preferred(&rows, "");
            let resolved = resolver.resolve(preferred, &mut BTreeSet::new());
            let properties = resolved.as_ref().unwrap_or(&preferred.export.properties);
            let mut entry = describe(preferred, &resolver.graph, properties);
            let mut error = resolved.as_ref().err().cloned();
            if error.is_none() {
                for row in &rows {
                    match resolver.resolve(*row, &mut BTreeSet::new()) {
                        Ok(other) => {
                            if let Some(difference) = definition_difference(
                                preferred,
                                properties,
                                *row,
                                &other,
                                &resolver.graph,
                            ) {
                                error = Some(format!(
                                    "Conflicting package definitions share this runtime object path: {difference} differs."
                                ));
                                break;
                            }
                        }
                        Err(reason) => {
                            error = Some(reason);
                            break;
                        }
                    }
                }
            }
            if let Some(reason) = error {
                entry.unavailable_reason = Some(reason);
                entry.capabilities = ItemCapabilities::default();
            }
            entry.source_packages = rows
                .iter()
                .map(|row| row.package.replace('\\', "/"))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            entry
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClassDefinition, PackageSource};
    use serde_json::json;

    fn props(value: Value) -> Properties {
        serde_json::from_value(value).unwrap()
    }
    fn export(path: &str, class: &str, values: Value) -> ExportRecord {
        ExportRecord {
            path: path.into(),
            class_name: class.into(),
            archetype: None,
            is_archetype: true,
            is_class_default: false,
            properties: props(values),
            property_error: None,
        }
    }
    fn package(exports: Vec<ExportRecord>) -> PackageFacts {
        let mut defaults = export(
            "Default__HeroEquipment",
            EQUIPMENT,
            json!({"bCanBeEquipped":true}),
        );
        defaults.is_class_default = true;
        PackageFacts {
            source: PackageSource {
                path: "UDKGame/CookedPCConsole/UDKGame.upk".into(),
                sha256: "a".repeat(64),
                bytes: 1,
                modified_ns: 0,
            },
            classes: vec![ClassDefinition {
                path: EQUIPMENT.into(),
                parent: Some(NATIVE_EQUIPMENT.into()),
            }],
            exports: [vec![defaults], exports].concat(),
        }
    }

    #[test]
    fn unknown_types_stay_visible_with_no_capabilities() {
        let catalog = classify(&[package(vec![export(
            "Items.Unknown",
            EQUIPMENT,
            json!({"EquipmentType":42}),
        )])]);
        assert_eq!(catalog[0].kind, ItemKind::Unknown);
        assert_eq!(catalog[0].capabilities, ItemCapabilities::default());
        assert!(catalog[0].unavailable_reason.is_some());
    }

    #[test]
    fn names_cannot_authorize_an_unfamiliar_class() {
        let class = "Mod.HeroEquipment_Familiar_New";
        let mut data = package(vec![export("Items.Pet", class, json!({"EquipmentType":5}))]);
        data.classes.push(ClassDefinition {
            path: class.into(),
            parent: Some(FAMILIAR.into()),
        });
        let catalog = classify(&[data]);
        assert_eq!(catalog[0].kind, ItemKind::Pet);
        assert!(catalog[0].unavailable_reason.is_some());
        assert_eq!(
            maximum_capabilities(ItemKind::Pet, class),
            ItemCapabilities::default()
        );
    }

    #[test]
    fn defaults_and_archetypes_resolve_before_category() {
        let base = export(
            "Items.Armor",
            EQUIPMENT,
            json!({"EquipmentType":"EQT_ARMOR_BOOTS"}),
        );
        let mut child = export(
            "Items.Child",
            EQUIPMENT,
            json!({"UserEquipmentName":"Child"}),
        );
        child.archetype = Some(base.path.clone());
        let catalog = classify(&[package(vec![base, child])]);
        assert_eq!(catalog[1].kind, ItemKind::Armor);
        assert_eq!(catalog[1].group, "Boots");
        assert!(catalog[1].capabilities.resistances);
    }

    #[test]
    fn same_slot_does_not_turn_collectibles_into_pets() {
        let item = export(
            "Resources.Coal_Equipment",
            EQUIPMENT,
            json!({"EquipmentType":5,"bCanBeEquipped":false,"DroppedEquipmentTemplates":["Resources.Drop"]}),
        );
        let catalog = classify(&[package(vec![item])]);
        assert_eq!(catalog[0].kind, ItemKind::Collectible);
        assert!(catalog[0].capabilities.pet.is_none());
        assert!(!catalog[0].capabilities.custom_common);
        assert_eq!(catalog[0].group, "Resources");
    }

    #[test]
    fn unknown_collectibles_are_not_assumed_to_be_exchange_tokens() {
        let item = export(
            "Items.FutureToken",
            EQUIPMENT,
            json!({"EquipmentType":5,"bCanBeEquipped":false,"DroppedEquipmentTemplates":["Items.Drop"]}),
        );
        let catalog = classify(&[package(vec![item])]);
        assert_eq!(catalog[0].group, "Unknown collectibles");
        assert_eq!(catalog[0].capabilities, ItemCapabilities::default());
    }

    #[test]
    fn deduplicate_localized_names_but_reject_gameplay_conflicts() {
        let first = package(vec![export(
            "Items.Sword",
            EQUIPMENT,
            json!({"UserEquipmentName":"Sword","WeaponDamageBonus":10}),
        )]);
        let mut second = first.clone();
        second.source.path = "UDKGame/CookedPCConsole/Startup_INT.upk".into();
        second.exports[1]
            .properties
            .insert("UserEquipmentName".into(), json!("English Sword"));
        let catalog = classify(&[first.clone(), second.clone()]);
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].name, "English Sword");
        assert!(catalog[0].unavailable_reason.is_none());
        assert_eq!(catalog[0].source_packages.len(), 2);
        second.exports[1]
            .properties
            .insert("WeaponDamageBonus".into(), json!(20));
        assert!(
            classify(&[first, second])[0]
                .unavailable_reason
                .as_ref()
                .unwrap()
                .contains("Conflicting")
        );
    }

    #[test]
    fn package_local_variants_keep_distinct_dependencies() {
        let variants: Vec<_> = ["EmeraldCity", "EmeraldCity_Small"]
            .into_iter()
            .map(|root| {
                let mut source = package(vec![export(
                    &format!("{root}.01Phoenix"),
                    EQUIPMENT,
                    json!({"EquipmentWeaponTemplate":format!("{root}.Weapon")}),
                )]);
                source.source.path = format!("UDKGame/CookedPCConsole/{root}.upk");
                source
            })
            .collect();
        let entries = classify(&variants);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.unavailable_reason.is_none() && entry.source_packages.len() == 1));
        let mut incompatible = variants[1].clone();
        incompatible.exports[1].path = variants[0].exports[1].path.clone();
        let entries = classify(&[variants[0].clone(), incompatible]);
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0]
                .unavailable_reason
                .as_ref()
                .unwrap()
                .contains("EquipmentWeaponTemplate")
        );
    }

    #[test]
    fn runtime_identity_is_case_insensitive_but_display_spelling_is_preserved() {
        let first = package(vec![export("Items.Sword", EQUIPMENT, json!({}))]);
        let mut second = first.clone();
        second.source.path = "UDKGame/CookedPCConsole/Startup_INT.upk".into();
        second.exports[1].path = "ITEMS.Sword".into();
        let entries = classify(&[first, second]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "ITEMS.Sword");
        assert!(entries[0].unavailable_reason.is_none());
        assert_eq!(entries[0].source_packages.len(), 2);
    }

    #[test]
    fn cooked_template_ids_do_not_change_equipment_compatibility_or_source_facts() {
        let first = package(vec![export(
            "Items.Sword",
            EQUIPMENT,
            json!({"equipmentTemplateUniqueID":i32::MIN,"WeaponDamageBonus":10}),
        )]);
        let mut second = first.clone();
        second.source.path = "UDKGame/CookedPCConsole/Startup_INT.upk".into();
        second.exports[1]
            .properties
            .insert("equipmentTemplateUniqueID".into(), json!(i32::MAX));
        let packages = [first, second];
        let entries = classify(&packages);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].unavailable_reason.is_none());
        assert!(entries[0].capabilities.template_defaults);
        assert_eq!(entries[0].source_packages.len(), 2);
        assert!(
            entries[0]
                .classification_rule
                .starts_with(&format!("v{}/", crate::RULES_VERSION))
        );
        for (data, id) in packages.iter().zip([i32::MIN, i32::MAX]) {
            assert_eq!(data.exports[1].properties["equipmentTemplateUniqueID"], id);
        }
    }

    #[test]
    fn template_id_differences_do_not_hide_incompatible_equipment_definitions() {
        for (field, before, after) in [
            ("WeaponDamageBonus", json!(10), json!(20)),
            (
                "EquipmentWeaponTemplate",
                json!("MapA.Weapon"),
                json!("MapB.Weapon"),
            ),
            ("bCantSave", json!(false), json!(true)),
            ("bForceUseParentTemplate", json!(false), json!(true)),
            ("EquipmentID1", json!(0), json!(42)),
        ] {
            let mut first = package(vec![export(
                "Items.Sword",
                EQUIPMENT,
                json!({"equipmentTemplateUniqueID":11}),
            )]);
            first.exports[1].properties.insert(field.into(), before);
            let mut second = first.clone();
            second.source.path = "UDKGame/CookedPCConsole/Other.upk".into();
            second.exports[1]
                .properties
                .insert("equipmentTemplateUniqueID".into(), json!(22));
            second.exports[1].properties.insert(field.into(), after);
            let entries = classify(&[first, second]);
            let reason = entries[0].unavailable_reason.as_deref().unwrap();
            assert!(reason.contains("Conflicting") && reason.contains(field));
            assert_eq!(entries[0].capabilities, ItemCapabilities::default());
        }
    }

    #[test]
    fn inherited_template_ids_are_ignored_but_parent_forcing_is_preserved() {
        let mut first = package(vec![export(
            "Items.ForcedParent",
            EQUIPMENT,
            json!({"bForceUseParentTemplate":true}),
        )]);
        first.exports[0]
            .properties
            .insert("equipmentTemplateUniqueID".into(), json!(11));
        let mut second = first.clone();
        second.source.path = "UDKGame/CookedPCConsole/Other.upk".into();
        second.exports[0]
            .properties
            .insert("equipmentTemplateUniqueID".into(), json!(22));
        let entries = classify(&[first.clone(), second.clone()]);
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0]
                .unavailable_reason
                .as_deref()
                .unwrap()
                .contains("forces its parent")
        );
        assert_eq!(entries[0].capabilities, ItemCapabilities::default());

        for data in [&mut first, &mut second] {
            data.exports[1].properties.remove("bForceUseParentTemplate");
        }
        let entries = classify(&[first, second]);
        assert!(entries[0].unavailable_reason.is_none());
        assert!(entries[0].capabilities.template_defaults);
    }

    #[test]
    fn invalid_template_id_values_are_not_discarded_as_harmless() {
        for id in [json!("22"), json!(i64::MAX), json!(1.5), Value::Null] {
            let first = package(vec![export(
                "Items.Sword",
                EQUIPMENT,
                json!({"equipmentTemplateUniqueID":11}),
            )]);
            let mut second = first.clone();
            second.source.path = "UDKGame/CookedPCConsole/Other.upk".into();
            second.exports[1]
                .properties
                .insert("equipmentTemplateUniqueID".into(), id);
            let entries = classify(&[first, second]);
            assert!(
                entries[0]
                    .unavailable_reason
                    .as_deref()
                    .unwrap()
                    .contains("equipmentTemplateUniqueID")
            );
            assert_eq!(entries[0].capabilities, ItemCapabilities::default());
        }
    }

    #[test]
    fn matching_values_do_not_merge_different_archetype_dependencies() {
        let mut item = export(
            "Items.Sword",
            EQUIPMENT,
            json!({"equipmentTemplateUniqueID":11}),
        );
        item.archetype = Some("Items.ParentA".into());
        let first = package(vec![
            export("Items.ParentA", EQUIPMENT, json!({"WeaponDamageBonus":10})),
            export("Items.ParentB", EQUIPMENT, json!({"WeaponDamageBonus":10})),
            item,
        ]);
        let mut second = first.clone();
        second.source.path = "UDKGame/CookedPCConsole/Other.upk".into();
        second.exports[3].archetype = Some("Items.ParentB".into());
        second.exports[3]
            .properties
            .insert("equipmentTemplateUniqueID".into(), json!(22));
        let entries = classify(&[first, second]);
        let sword = entries
            .iter()
            .find(|entry| entry.path == "Items.Sword")
            .unwrap();
        assert!(
            sword
                .unavailable_reason
                .as_deref()
                .unwrap()
                .contains("archetype reference")
        );
        assert_eq!(sword.capabilities, ItemCapabilities::default());
    }

    #[test]
    fn equipment_id_exception_does_not_apply_to_world_pickup_fields() {
        let class = "UDKGame.DunDefManaToken";
        let mut default = export("Default__DunDefManaToken", class, json!({}));
        default.is_class_default = true;
        let first = package(vec![
            default,
            export(
                "Items.Token",
                class,
                json!({"equipmentTemplateUniqueID":11}),
            ),
        ]);
        let mut second = first.clone();
        second.source.path = "UDKGame/CookedPCConsole/Other.upk".into();
        second.exports[2]
            .properties
            .insert("equipmentTemplateUniqueID".into(), json!(22));
        let entries = classify(&[first, second]);
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0]
                .unavailable_reason
                .as_deref()
                .unwrap()
                .contains("equipmentTemplateUniqueID")
        );
        assert_eq!(entries[0].capabilities, ItemCapabilities::default());
    }

    #[test]
    fn missing_archetypes_and_cycles_are_unavailable() {
        let mut first = export("Items.First", EQUIPMENT, json!({}));
        first.archetype = Some("Items.Missing".into());
        assert!(
            classify(&[package(vec![first.clone()])])[0]
                .unavailable_reason
                .is_some()
        );
        first.archetype = Some("Items.Second".into());
        let mut second = export("Items.Second", EQUIPMENT, json!({}));
        second.archetype = Some("Items.First".into());
        assert!(
            classify(&[package(vec![first, second])])
                .iter()
                .all(|entry| entry.unavailable_reason.is_some())
        );
    }

    #[test]
    fn runtime_capabilities_restrict_family_fields_and_token_value() {
        let stats = capabilities(
            ItemKind::Pet,
            "UDKGame.HeroEquipment_Familiar_Melee",
            &props(json!({"UseWeaponCoreStats":true,"bUseFixedShootSpeed":true})),
        );
        let pet = stats.pet.unwrap();
        assert!(pet.damage);
        assert!(!pet.attack_rate);
        assert!(!pet.projectiles);
        let coal = props(json!({"bUseHighDigitManaTokenValue":true}));
        assert!(!capabilities(ItemKind::Collectible, EQUIPMENT, &coal).stored_mana);
        let token = props(
            json!({"bUseHighDigitManaTokenValue":true,"bPlayerShopForceToMinimumSellWorth":true}),
        );
        assert!(capabilities(ItemKind::Collectible, EQUIPMENT, &token).stored_mana);
        assert_eq!(
            category_from_runtime(EQUIPMENT, 5, false),
            ItemKind::Collectible
        );
        assert_eq!(category_from_runtime(EQUIPMENT, 5, true), ItemKind::Unknown);
    }

    #[test]
    fn flags_and_unsafe_defaults_stay_blocked() {
        let mut data = package(vec![
            export("Items.NoSave", EQUIPMENT, json!({"bCantSave":true})),
            export("Flags.Red", "DunDefSpecial.CTF_Flag", json!({})),
        ]);
        let mut default = export(
            "DunDefSpecial.Default__CTF_Flag",
            "DunDefSpecial.CTF_Flag",
            json!({}),
        );
        default.is_class_default = true;
        data.exports.push(default);
        let catalog = classify(&[data]);
        assert_eq!(catalog.len(), 2);
        assert!(
            catalog
                .iter()
                .all(|entry| entry.unavailable_reason.is_some())
        );
        assert!(
            catalog
                .iter()
                .all(|entry| entry.capabilities == ItemCapabilities::default())
        );
    }

    #[test]
    fn conflicting_inherited_defaults_block_single_copy_candidates() {
        let first = package(vec![export("Items.OneCopy", EQUIPMENT, json!({}))]);
        let mut second = package(vec![]);
        second.source.path = "UDKGame/CookedPCConsole/Startup_INT.upk".into();
        second.exports[0]
            .properties
            .insert("WeaponDamageBonus".into(), json!(999));
        let entries = classify(&[first, second]);
        assert_eq!(entries.len(), 1);
        assert!(
            entries[0]
                .unavailable_reason
                .as_ref()
                .unwrap()
                .contains("inherited object")
        );
        assert_eq!(entries[0].capabilities, ItemCapabilities::default());
    }

    #[test]
    fn unknown_class_defaults_and_inheritance_cycles_cannot_be_admitted() {
        let mut missing = package(vec![export("Items.MissingDefaults", EQUIPMENT, json!({}))]);
        missing.exports.remove(0);
        assert!(
            classify(&[missing])[0]
                .unavailable_reason
                .as_ref()
                .unwrap()
                .contains("Class defaults")
        );
        let mut cyclic = package(vec![export("Items.Cycle", EQUIPMENT, json!({}))]);
        cyclic.classes[0].parent = Some(EQUIPMENT.into());
        assert!(
            classify(&[cyclic])[0]
                .unavailable_reason
                .as_ref()
                .unwrap()
                .contains("cyclic")
        );
    }

    #[test]
    fn unqualified_identities_are_not_invented_and_distinct_variants_remain() {
        let data = package(vec![
            export("Unqualified", EQUIPMENT, json!({})),
            export(
                "Items.Crossbow_Boss_LD",
                EQUIPMENT,
                json!({"weaponType":"EWT_WEAPON_INITIATE"}),
            ),
            export(
                "Items.Crossbow_Ruthless",
                EQUIPMENT,
                json!({"weaponType":"EWT_WEAPON_INITIATE"}),
            ),
        ]);
        let entries = classify(&[data]);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].variants, ["LD", "Boss"]);
        assert_eq!(entries[1].variants, ["Ruthless"]);
        assert_eq!(entries[2].path, "Unqualified");
        assert!(
            entries[2]
                .unavailable_reason
                .as_ref()
                .unwrap()
                .contains("Unqualified")
        );
    }

    #[test]
    fn actual_parent_forcing_and_disabled_randomization_are_respected() {
        let entries = classify(&[package(vec![
            export(
                "Items.ForcedParent",
                EQUIPMENT,
                json!({"bForceUseParentTemplate":true}),
            ),
            export(
                "Items.Fixed",
                EQUIPMENT,
                json!({"bDisableTheRandomization":true}),
            ),
        ])]);
        assert!(entries[0].capabilities.template_defaults);
        assert!(!entries[0].capabilities.random_roll);
        assert_eq!(entries[1].capabilities, ItemCapabilities::default());
        assert!(
            entries[1]
                .unavailable_reason
                .as_ref()
                .unwrap()
                .contains("forces its parent")
        );
    }
}
