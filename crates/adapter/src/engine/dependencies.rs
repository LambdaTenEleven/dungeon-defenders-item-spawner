//! Validate the worker's source-derived weapon graph against live engine objects.
use super::{Engine, NAMES, Result, name, read, require, u32_at, word};
use dd_protocol::CatalogEntry;

fn live_path(engine: &Engine, mut object: u32) -> Result<String> {
    let names = read(engine.base + NAMES, 12)?;
    require(
        word(&names, 4) <= word(&names, 8) && word(&names, 8) <= 500_000,
        "Invalid dependency name table.",
    )?;
    let mut parts = Vec::new();
    for _ in 0..32 {
        if object == 0 {
            parts.reverse();
            return Ok(parts.join("."));
        }
        engine.live_object(object)?;
        let data = read(object, 0x38)?;
        let index = word(&data, 0x2c);
        require(index < word(&names, 4), "Invalid dependency name index.")?;
        let mut label = name(u32_at(word(&names, 0) + index * 4)?)?;
        let number = word(&data, 0x30);
        if number != 0 {
            label.push_str(&format!("_{}", number - 1));
        }
        parts.push(label);
        object = word(&data, 0x28);
    }
    Err("Dependency outer cycle/limit.".into())
}

pub(super) fn validate(engine: &Engine, entry: &CatalogEntry, template: u32) -> Result<()> {
    let graph = engine
        .dependencies
        .as_ref()
        .ok_or("Weapon dependencies were not prepared from verified source packages.")?;
    require(
        graph.template == entry.path && !graph.nodes.is_empty() && graph.nodes.len() <= 64,
        "Prepared dependency graph differs from the selected item.",
    )?;
    let mut objects = vec![None; graph.nodes.len()];
    objects[0] = Some(template);
    for (index, node) in graph.nodes.iter().enumerate() {
        let object = objects[index].ok_or("Disconnected dependency graph.")?;
        engine.live_object(object)?;
        require(
            live_path(engine, object)?.eq_ignore_ascii_case(&node.path),
            &format!(
                "Loaded dependency identity differs from source: {}",
                node.path
            ),
        )?;
        require(
            u32_at(object + 0x34)? == engine.reference(&node.class)?,
            &format!("Loaded dependency class differs from source: {}", node.path),
        )?;
        for link in &node.links {
            let rule = &link.rule;
            let required = rule.required
                || (rule.role == dd_catalog::dependencies::Role::Projectile
                    && [
                        "UDKGame.DunDefWeapon_Crossbow",
                        "UDKGame.DunDefWeapon_MagicStaff",
                    ]
                    .into_iter()
                    .try_fold(false, |matched, class| -> Result<bool> {
                        Ok(matched
                            || Engine::class_inherits(
                                u32_at(object + 0x34)?,
                                engine.reference(class)?,
                            )?)
                    })?);
            engine.is_a(
                object,
                rule.property
                    .rsplit_once('.')
                    .ok_or("Invalid dependency property")?
                    .0,
            )?;
            let field = engine.reflect_field(
                rule.property,
                if rule.component {
                    "Core.ComponentProperty"
                } else {
                    "Core.ObjectProperty"
                },
                4,
            )?;
            require(
                field.count == 1
                    && u32_at(engine.reference(rule.property)? + 0x84)?
                        == engine.reference(rule.target)?,
                &format!("Dependency field contract changed: {}", rule.property),
            )?;
            let value = u32_at(object + field.offset)?;
            match link.target {
                Some(target) => {
                    engine.is_a(value, rule.target).map_err(|error| {
                        format!(
                            "Invalid dependency {} on {}: {error}",
                            rule.property, node.path
                        )
                    })?;
                    if rule.component {
                        require(
                            u32_at(value + 0x28)? == object,
                            &format!(
                                "Dependency component owner differs: {} on {}",
                                rule.property, node.path
                            ),
                        )?;
                    }
                    require(
                        target > index && target < objects.len() && objects[target].is_none(),
                        "Invalid dependency graph edge.",
                    )?;
                    objects[target] = Some(value);
                }
                None => require(
                    value == 0 && !required,
                    &format!(
                        "Optional dependency differs from source: {} on {}",
                        rule.property, node.path
                    ),
                )?,
            }
        }
    }
    Ok(())
}
