//! Bundled equipment templates and the list filters over them.
//!
//! The hero column in `equipment.tsv` is each weapon's serialized `HeroEquipment.weaponType`
//! (following parent archetypes), read from the cooked Startup package for this game build. The game
//! matches it against `DunDefHero.HeroWeaponType`: Apprentice, Squire, Initiate (Huntress) and
//! Recruit (Monk).
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Weapon,
    Armor,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Weapon => "Weapon",
            Kind::Armor => "Armor",
        }
    }

    /// Filter chips for the hero (weapon) or slot (armor) column.
    pub fn groups(self) -> &'static [&'static str] {
        match self {
            Kind::Weapon => &["Squire", "Apprentice", "Huntress", "Monk"],
            Kind::Armor => &["Helmet", "Torso", "Gauntlets", "Boots", "Other"],
        }
    }

    pub fn group_title(self) -> &'static str {
        match self {
            Kind::Weapon => "Hero",
            Kind::Armor => "Slot",
        }
    }

    /// Filter chips for tags read from the template name.
    pub fn variants(self) -> &'static [&'static str] {
        match self {
            Kind::Weapon => &["Base", "LD", "Ruthless", "Boss", "EMH"],
            Kind::Armor => &["Leather", "Chain", "Mail", "Plate", "Pristine"],
        }
    }

    pub fn variant_title(self) -> &'static str {
        match self {
            Kind::Weapon => "Variant",
            Kind::Armor => "Material",
        }
    }
}

pub struct Template {
    pub name: &'static str,
    pub kind: Kind,
    /// Hero class for weapons, equipment slot for armor.
    pub group: &'static str,
    pub path: &'static str,
    pub variants: Vec<&'static str>,
    /// Group and variants for display, such as "Huntress · LD · Boss".
    pub tags: String,
    search_key: String,
}

pub fn templates() -> Vec<Template> {
    parse(include_str!("equipment.tsv"))
}

fn parse(source: &'static str) -> Vec<Template> {
    source
        .lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let kind = match columns.next()? {
                "Weapon" => Kind::Weapon,
                "Armor" => Kind::Armor,
                _ => return None,
            };
            let group = columns.next()?;
            let path = columns.next()?;
            let leaf = path.rsplit('.').next()?;
            let name = leaf
                .strip_prefix("HeroEquipment_")
                .or_else(|| leaf.strip_prefix("Equipment_"))
                .unwrap_or(leaf);
            let variants = variants(kind, leaf);
            let mut tags = vec![group];
            tags.extend(&variants);
            Some(Template {
                name,
                kind,
                group,
                path,
                variants,
                tags: tags.join(" · "),
                search_key: format!("{path} {} {group}", kind.label()).to_lowercase(),
            })
        })
        .collect()
}

/// Name tokens such as `Boss`, `EMHv2` or `Ruthless5` start with a tag. `LD` must match exactly.
fn has_tag(leaf: &str, tag: &str) -> bool {
    leaf.split('_').any(|token| {
        if tag == "LD" {
            token.eq_ignore_ascii_case(tag)
        } else {
            token
                .get(..tag.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(tag))
        }
    })
}

fn variants(kind: Kind, leaf: &str) -> Vec<&'static str> {
    let tags = match kind {
        Kind::Weapon => &kind.variants()[1..],
        Kind::Armor => kind.variants(),
    };
    let found: Vec<_> = tags
        .iter()
        .copied()
        .filter(|tag| has_tag(leaf, tag))
        .collect();
    match kind {
        Kind::Weapon if found.is_empty() => vec!["Base"],
        _ => found,
    }
}

/// Chips within one row match any selected value; rows and the search text must all match.
#[derive(Clone, Default, PartialEq)]
pub struct Filter {
    pub text: String,
    pub kind: Option<Kind>,
    pub groups: BTreeSet<&'static str>,
    pub variants: BTreeSet<&'static str>,
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
            && (self.groups.is_empty() || self.groups.contains(item.group))
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
mod tests {
    use super::*;

    fn find<'a>(templates: &'a [Template], leaf: &str) -> &'a Template {
        templates
            .iter()
            .find(|item| item.path.ends_with(leaf))
            .unwrap()
    }

    #[test]
    fn every_catalog_row_parses_into_a_known_group() {
        let templates = templates();
        assert_eq!(
            templates.len(),
            include_str!("equipment.tsv").lines().count()
        );
        for item in &templates {
            assert!(item.kind.groups().contains(&item.group), "{}", item.path);
            if item.kind == Kind::Weapon {
                assert!(!item.variants.is_empty(), "{}", item.path);
            }
        }
    }

    #[test]
    fn variants_and_materials_come_from_name_tokens() {
        let templates = templates();
        let boss = find(&templates, ".CrossbowStyle8_Boss_LD");
        assert_eq!(boss.variants, ["LD", "Boss"]);
        assert_eq!(boss.group, "Huntress");
        let emh = find(&templates, ".MagicStaffStyle4_EMHv2");
        assert_eq!(emh.variants, ["EMH"]);
        let ruthless = find(&templates, ".HeroEquipment_Monk_Ruthless5");
        assert_eq!(ruthless.variants, ["Ruthless"]);
        let embers = find(&templates, ".HeroEquipment_SwordOfEmbers");
        assert_eq!((embers.name, embers.group), ("SwordOfEmbers", "Squire"));
        assert_eq!(embers.variants, ["Base"]);
        // Game data, not the name: NorthPole is an Apprentice staff, the harpoon a Huntress weapon.
        assert_eq!(
            find(&templates, ".HeroEquipment_NorthPole").group,
            "Apprentice"
        );
        assert_eq!(
            find(&templates, ".HeroEquipment_BucBayHarpoon").group,
            "Huntress"
        );
        let boots = find(&templates, ".BootsArmorBase_Leather_Squanto");
        assert_eq!(boots.group, "Boots");
        assert_eq!(boots.variants, ["Leather"]);
    }

    #[test]
    fn filters_combine_kind_groups_variants_and_text() {
        let templates = templates();
        let mut filter = Filter::default();
        assert_eq!(filter.apply(&templates).len(), templates.len());

        filter.set_kind(Some(Kind::Armor));
        let armor = filter.apply(&templates);
        assert!(armor.iter().all(|&i| templates[i].kind == Kind::Armor));
        filter.groups.insert("Boots");
        filter.variants.insert("Plate");
        filter.variants.insert("Chain");
        let names = |indices: Vec<usize>| -> Vec<&str> {
            indices.into_iter().map(|i| templates[i].name).collect()
        };
        assert_eq!(
            names(filter.apply(&templates)),
            ["BootsArmorBase_Chain", "BootsArmorBase_Plate"]
        );

        filter.set_kind(Some(Kind::Weapon));
        assert!(filter.groups.is_empty() && filter.variants.is_empty());
        filter.groups.insert("Monk");
        filter.text = " ARABIA ".into();
        let monk = filter.apply(&templates);
        assert!(!monk.is_empty());
        for i in monk {
            assert_eq!(templates[i].group, "Monk");
            assert!(templates[i].path.to_lowercase().contains("arabia"));
        }
        assert!(filter.is_active());
    }
}
