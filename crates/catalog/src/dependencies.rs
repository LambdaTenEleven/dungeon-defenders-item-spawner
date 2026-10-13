//! Bounded weapon relationships. Object identities come from packages, never item names.
use crate::ExportRecord;
use std::collections::BTreeSet;

type Result<T> = std::result::Result<T, String>;
const LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Equipment,
    Weapon,
    Mesh,
    Projectile,
    Emitter,
    Particles,
    Asset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub property: &'static str,
    pub target: &'static str,
    pub role: Role,
    pub component: bool,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub rule: Rule,
    pub target: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub path: String,
    pub class: String,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependencies {
    pub template: String,
    pub nodes: Vec<Node>,
}

fn rules(role: Role, class: &str) -> Result<Vec<Rule>> {
    use Role::*;
    let rule = |property, target, role, component, required| Rule {
        property,
        target,
        role,
        component,
        required,
    };
    Ok(match role {
        Equipment => vec![rule(
            "UDKGame.HeroEquipment.EquipmentWeaponTemplate",
            "UDKGame.DunDefWeapon",
            Weapon,
            false,
            true,
        )],
        Weapon => vec![
            rule(
                "Engine.Weapon.Mesh",
                "Engine.MeshComponent",
                Mesh,
                true,
                true,
            ),
            rule(
                "UDKGame.DunDefWeapon.ProjectileTemplate",
                "UDKGame.DunDefProjectile",
                Projectile,
                false,
                class == "UDKGame.DunDefWeapon_Crossbow",
            ),
        ],
        Mesh => vec![match class {
            "Engine.SkeletalMeshComponent" => rule(
                "Engine.SkeletalMeshComponent.SkeletalMesh",
                "Engine.SkeletalMesh",
                Asset,
                false,
                true,
            ),
            "Engine.StaticMeshComponent" => rule(
                "Engine.StaticMeshComponent.StaticMesh",
                "Engine.StaticMesh",
                Asset,
                false,
                true,
            ),
            _ => return Err(format!("Unsupported weapon mesh component class: {class}")),
        }],
        Projectile => vec![
            rule(
                "Engine.Actor.CollisionComponent",
                "Engine.PrimitiveComponent",
                Asset,
                true,
                true,
            ),
            rule(
                "UDKGame.DunDefProjectile.ProjectileEmitterTemplate",
                "UDKGame.DunDefEmitterSpawnable",
                Emitter,
                false,
                false,
            ),
            rule(
                "UDKGame.DunDefProjectile.ImpactEmitterTemplate",
                "UDKGame.DunDefEmitterSpawnable",
                Emitter,
                false,
                false,
            ),
        ],
        Emitter => vec![rule(
            "Engine.Emitter.ParticleSystemComponent",
            "Engine.ParticleSystemComponent",
            Particles,
            true,
            true,
        )],
        Particles => vec![rule(
            "Engine.ParticleSystemComponent.Template",
            "Engine.ParticleSystem",
            Asset,
            false,
            false,
        )],
        Asset => vec![],
    })
}

/// The reader must return source-verified exports, including inherited defaults.
/// Missing/malformed inheritance is an error; only an established absent/null
/// optional reference may be omitted. No arbitrary property graph is followed.
pub fn build(
    template: &str,
    mut read: impl FnMut(&str, bool) -> Result<ExportRecord>,
) -> Result<Dependencies> {
    fn property(
        record: &ExportRecord,
        leaf: &str,
        read: &mut impl FnMut(&str, bool) -> Result<ExportRecord>,
    ) -> Result<Option<String>> {
        let mut record = record.clone();
        let mut seen = BTreeSet::new();
        loop {
            if seen.len() >= LIMIT || !seen.insert(record.path.to_ascii_lowercase()) {
                return Err(format!(
                    "Dependency inheritance cycle/limit: {}",
                    record.path
                ));
            }
            if let Some(error) = &record.property_error {
                return Err(format!("Unresolved dependency {}: {error}", record.path));
            }
            if let Some(value) = record.properties.get(leaf) {
                return match value {
                    serde_json::Value::Null => Ok(None),
                    serde_json::Value::String(path) if path.contains('.') && path.len() <= 8192 => {
                        Ok(Some(path.clone()))
                    }
                    _ => Err(format!(
                        "Invalid dependency reference: {}.{leaf}",
                        record.path
                    )),
                };
            }
            let Some(parent) = &record.archetype else {
                return Ok(None);
            };
            record = read(parent, true)?;
        }
    }
    fn visit(
        path: &str,
        role: Role,
        graph: &mut Dependencies,
        active: &mut BTreeSet<String>,
        read: &mut impl FnMut(&str, bool) -> Result<ExportRecord>,
    ) -> Result<usize> {
        if graph.nodes.len() >= LIMIT || !active.insert(path.to_ascii_lowercase()) {
            return Err(format!("Dependency graph cycle/limit: {path}"));
        }
        let record = read(path, role != Role::Asset)?;
        if !record.path.eq_ignore_ascii_case(path) {
            return Err(format!("Dependency identity differs: {path}"));
        }
        let index = graph.nodes.len();
        graph.nodes.push(Node {
            path: record.path.clone(),
            class: record.class_name.clone(),
            links: vec![],
        });
        for rule in rules(role, &record.class_name)? {
            let leaf = rule.property.rsplit('.').next().unwrap();
            let value = property(&record, leaf, read)?;
            if value.is_none() && rule.required {
                return Err(format!(
                    "Required dependency missing: {}.{leaf}",
                    record.path
                ));
            }
            let target = value
                .map(|path| visit(&path, rule.role, graph, active, read))
                .transpose()?;
            graph.nodes[index].links.push(Link { rule, target });
        }
        active.remove(&path.to_ascii_lowercase());
        Ok(index)
    }
    let mut graph = Dependencies {
        template: template.into(),
        nodes: vec![],
    };
    visit(
        template,
        Role::Equipment,
        &mut graph,
        &mut BTreeSet::new(),
        &mut read,
    )?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn fixture(root: &str) -> BTreeMap<String, ExportRecord> {
        [
            (
                "Item",
                "UDKGame.HeroEquipment",
                json!({"EquipmentWeaponTemplate":format!("{root}.Weapon")}),
            ),
            (
                "Weapon",
                "UDKGame.DunDefWeapon_Sword",
                json!({"Mesh":format!("{root}.Weapon.Mesh"),"ProjectileTemplate":null}),
            ),
            (
                "Weapon.Mesh",
                "Engine.SkeletalMeshComponent",
                json!({"SkeletalMesh":format!("{root}.MeshAsset")}),
            ),
            ("MeshAsset", "Engine.SkeletalMesh", json!({})),
        ]
        .into_iter()
        .map(|(leaf, class, props)| {
            let path = format!("{root}.{leaf}");
            (
                path.clone(),
                ExportRecord {
                    path,
                    class_name: class.into(),
                    archetype: None,
                    is_archetype: true,
                    is_class_default: false,
                    properties: serde_json::from_value(props).unwrap(),
                    property_error: None,
                },
            )
        })
        .collect()
    }

    fn graph(records: &BTreeMap<String, ExportRecord>, root: &str) -> Result<Dependencies> {
        build(&format!("{root}.Item"), |path, _| {
            records
                .get(path)
                .cloned()
                .ok_or_else(|| format!("Unresolved: {path}"))
        })
    }

    #[test]
    fn arbitrary_package_names_drive_identities_and_optional_nulls() {
        for root in ["MapA", "CompletelyDifferent"] {
            let result = graph(&fixture(root), root).unwrap();
            assert_eq!(result.nodes.len(), 4);
            assert_eq!(result.nodes[1].links[1].target, None);
            assert!(result.nodes.iter().all(|node| node.path.starts_with(root)));
        }
        assert_ne!(
            graph(&fixture("A"), "A").unwrap(),
            graph(&fixture("B"), "B").unwrap()
        );
    }

    #[test]
    fn resolves_inherited_references_and_explicit_null_overrides() {
        let mut records = fixture("Test");
        let mut parent = records["Test.Weapon"].clone();
        parent.path = "Test.Parent".into();
        parent
            .properties
            .insert("ProjectileTemplate".into(), json!("Missing.Projectile"));
        records.insert(parent.path.clone(), parent);
        let child = records.get_mut("Test.Weapon").unwrap();
        child.archetype = Some("Test.Parent".into());
        child.properties.remove("Mesh");
        assert!(graph(&records, "Test").is_ok());
        records
            .get_mut("Test.Weapon")
            .unwrap()
            .properties
            .remove("ProjectileTemplate");
        assert!(
            graph(&records, "Test")
                .unwrap_err()
                .contains("Missing.Projectile")
        );
    }

    #[test]
    fn missing_malformed_and_unreviewed_dependencies_block() {
        for value in [
            json!(null),
            json!(42),
            json!("Unqualified"),
            json!("Missing.Mesh"),
        ] {
            let mut records = fixture("Test");
            records
                .get_mut("Test.Weapon")
                .unwrap()
                .properties
                .insert("Mesh".into(), value);
            assert!(graph(&records, "Test").is_err());
        }
        let mut records = fixture("Test");
        records.get_mut("Test.Weapon.Mesh").unwrap().class_name = "Unknown.MeshComponent".into();
        assert!(graph(&records, "Test").unwrap_err().contains("Unsupported"));
        records = fixture("Test");
        records.get_mut("Test.Weapon.Mesh").unwrap().property_error = Some("Truncated".into());
        assert!(graph(&records, "Test").unwrap_err().contains("Truncated"));
    }

    #[test]
    fn empty_effect_is_optional_but_unresolved_effect_is_not() {
        let mut records = fixture("Test");
        for (path, class, values) in [
            (
                "Projectile",
                "UDKGame.DunDefProjectile",
                json!({"CollisionComponent":"Test.Projectile.Collision","ProjectileEmitterTemplate":"Test.Effect","ImpactEmitterTemplate":null}),
            ),
            (
                "Projectile.Collision",
                "Engine.CylinderComponent",
                json!({}),
            ),
            (
                "Effect",
                "UDKGame.DunDefEmitterSpawnable",
                json!({"ParticleSystemComponent":"Test.Effect.Particles"}),
            ),
            (
                "Effect.Particles",
                "Engine.ParticleSystemComponent",
                json!({"Template":null}),
            ),
        ] {
            let path = format!("Test.{path}");
            records.insert(
                path.clone(),
                ExportRecord {
                    path,
                    class_name: class.into(),
                    archetype: None,
                    is_archetype: true,
                    is_class_default: false,
                    properties: serde_json::from_value(values).unwrap(),
                    property_error: None,
                },
            );
        }
        records
            .get_mut("Test.Weapon")
            .unwrap()
            .properties
            .insert("ProjectileTemplate".into(), json!("Test.Projectile"));
        let result = graph(&records, "Test").unwrap();
        assert_eq!(result.nodes.last().unwrap().links[0].target, None);
        records
            .get_mut("Test.Effect.Particles")
            .unwrap()
            .properties
            .insert("Template".into(), json!("Missing.Effect"));
        assert!(
            graph(&records, "Test")
                .unwrap_err()
                .contains("Missing.Effect")
        );
        let particles = records.get_mut("Test.Effect.Particles").unwrap();
        particles.properties.clear();
        particles.archetype = Some("Missing.Parent".into());
        assert!(
            graph(&records, "Test")
                .unwrap_err()
                .contains("Missing.Parent")
        );
    }

    #[test]
    fn dependency_and_inheritance_cycles_block() {
        let mut records = fixture("Test");
        records
            .get_mut("Test.Weapon.Mesh")
            .unwrap()
            .properties
            .insert("SkeletalMesh".into(), json!("Test.Item"));
        assert!(graph(&records, "Test").unwrap_err().contains("cycle"));
        records = fixture("Test");
        let item = records.get_mut("Test.Item").unwrap();
        item.properties.clear();
        item.archetype = Some(item.path.clone());
        assert!(
            graph(&records, "Test")
                .unwrap_err()
                .contains("inheritance cycle")
        );
    }

    #[test]
    fn same_path_different_class_or_reference_is_incompatible() {
        let records = fixture("Test");
        let original = graph(&records, "Test").unwrap();
        let mut changed = records.clone();
        changed.get_mut("Test.MeshAsset").unwrap().class_name = "Engine.StaticMesh".into();
        assert_ne!(original, graph(&changed, "Test").unwrap());
        changed = records.clone();
        let mut asset = changed["Test.MeshAsset"].clone();
        asset.path = "Test.AnotherAsset".into();
        changed.insert(asset.path.clone(), asset);
        changed
            .get_mut("Test.Weapon.Mesh")
            .unwrap()
            .properties
            .insert("SkeletalMesh".into(), json!("Test.AnotherAsset"));
        assert_ne!(original, graph(&changed, "Test").unwrap());
    }
}
