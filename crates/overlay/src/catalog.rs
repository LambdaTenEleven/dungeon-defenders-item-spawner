//! Presentation and filters over a catalog discovered from the selected installation.
use dd_catalog::CatalogEntry;
pub use dd_catalog::ItemKind as Kind;
use std::collections::BTreeSet;

pub const KINDS: [Kind; 7] = Kind::ALL;

pub struct Template {
    pub entry: CatalogEntry,
    pub tags: String,
    search_key: String,
}

impl std::ops::Deref for Template {
    type Target = CatalogEntry;
    fn deref(&self) -> &Self::Target {
        &self.entry
    }
}

pub fn templates(entries: &[CatalogEntry]) -> Vec<Template> {
    entries
        .iter()
        .map(|entry| {
            let mut tags = vec![entry.group.as_str()];
            tags.extend(entry.variants.iter().map(String::as_str));
            Template {
                entry: entry.clone(),
                tags: tags.join(" · "),
                search_key: format!(
                    "{} {} {} {} {}",
                    entry.name,
                    entry.path,
                    entry.kind.label(),
                    entry.group,
                    entry.variants.join(" ")
                )
                .to_lowercase(),
            }
        })
        .collect()
}

pub fn groups(kind: Kind, templates: &[Template]) -> Vec<String> {
    let mut groups: BTreeSet<String> = match kind {
        Kind::Weapon => ["Squire", "Apprentice", "Huntress", "Monk", "Other"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        Kind::Armor => ["Helmet", "Torso", "Gauntlets", "Boots", "Other"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        _ => BTreeSet::new(),
    };
    groups.extend(
        templates
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| entry.group.clone()),
    );
    groups.into_iter().collect()
}

pub fn variants(kind: Kind) -> Vec<String> {
    let variants: &[&str] = match kind {
        Kind::Weapon => &["Base", "LD", "Ruthless", "Boss", "EMH"],
        Kind::Armor => &["Leather", "Chain", "Mail", "Plate", "Pristine"],
        _ => &[],
    };
    variants.iter().map(|value| (*value).to_owned()).collect()
}

pub fn group_title(kind: Kind) -> &'static str {
    match kind {
        Kind::Weapon => "Hero",
        Kind::Armor | Kind::Accessory => "Slot",
        Kind::Pet | Kind::Collectible | Kind::WorldPickup | Kind::Unknown => "Family",
    }
}

/// Chips within one row match any selected value; rows and search must all match.
#[derive(Clone, Default, PartialEq)]
pub struct Filter {
    pub text: String,
    pub kind: Option<Kind>,
    pub groups: BTreeSet<String>,
    pub variants: BTreeSet<String>,
}

impl Filter {
    pub fn set_kind(&mut self, kind: Option<Kind>) {
        if self.kind != kind {
            self.kind = kind;
            self.groups.clear();
            self.variants.clear();
        }
    }
    pub fn is_active(&self) -> bool {
        *self != Self::default()
    }
    pub fn matches(&self, item: &Template) -> bool {
        self.kind.is_none_or(|kind| kind == item.kind)
            && (self.groups.is_empty() || self.groups.contains(item.group.as_str()))
            && (self.variants.is_empty() || item.variants.iter().any(|v| self.variants.contains(v)))
            && item.search_key.contains(&self.text.trim().to_lowercase())
    }
    pub fn apply(&self, templates: &[Template]) -> Vec<usize> {
        (0..templates.len())
            .filter(|&index| self.matches(&templates[index]))
            .collect()
    }
}

#[cfg(test)]
pub fn test_entry(kind: Kind) -> CatalogEntry {
    use dd_catalog::ItemCapabilities;
    CatalogEntry {
        name: format!("Example {}", kind.label()),
        path: format!("Package.{kind:?}"),
        class_name: "Game.Example".into(),
        source_packages: vec!["CookedPC/Example.upk".into()],
        kind,
        group: "Example family".into(),
        variants: vec![],
        equipment_type: None,
        capabilities: ItemCapabilities {
            template_defaults: kind != Kind::Unknown,
            random_roll: matches!(
                kind,
                Kind::Weapon | Kind::Armor | Kind::Pet | Kind::Accessory
            ),
            custom_common: matches!(
                kind,
                Kind::Weapon | Kind::Armor | Kind::Pet | Kind::Accessory
            ),
            weapon: kind == Kind::Weapon,
            resistances: kind == Kind::Armor,
            ..Default::default()
        },
        unavailable_reason: (kind == Kind::Unknown).then(|| "Unknown equipment type".into()),
        classification_rule: "test-fixture".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_discovered_kind_remains_visible_including_unknown() {
        let entries: Vec<_> = KINDS.into_iter().map(test_entry).collect();
        let templates = templates(&entries);
        assert_eq!(templates.len(), entries.len());
        for kind in KINDS {
            let item = templates.iter().find(|item| item.kind == kind).unwrap();
            assert!(groups(kind, &templates).contains(&item.group));
            let mut filter = Filter::default();
            filter.set_kind(Some(kind));
            filter.groups.insert(item.group.clone());
            filter.text = item.name.to_uppercase();
            assert!(filter.matches(item));
            filter.text = item.path.clone();
            assert!(filter.matches(item));
            assert_eq!(filter.apply(&templates).len(), 1);
        }
        assert!(
            templates
                .iter()
                .find(|item| item.kind == Kind::Unknown)
                .unwrap()
                .unavailable_reason
                .is_some()
        );
    }
    #[test]
    fn equipment_filters_preserve_hero_slot_and_variant_semantics() {
        let mut weapon = test_entry(Kind::Weapon);
        weapon.group = "Huntress".into();
        weapon.variants = vec!["LD".into(), "Boss".into()];
        let mut armor = test_entry(Kind::Armor);
        armor.group = "Boots".into();
        armor.variants = vec!["Plate".into()];
        let templates = templates(&[weapon, armor]);
        let mut filter = Filter::default();
        filter.set_kind(Some(Kind::Weapon));
        filter.groups.insert("Huntress".into());
        filter.variants.insert("LD".into());
        filter.variants.insert("Boss".into());
        assert_eq!(filter.apply(&templates), [0]);
        filter.set_kind(Some(Kind::Armor));
        assert!(filter.groups.is_empty() && filter.variants.is_empty());
        filter.groups.insert("Boots".into());
        filter.variants.insert("Plate".into());
        assert_eq!(filter.apply(&templates), [1]);
    }
}
