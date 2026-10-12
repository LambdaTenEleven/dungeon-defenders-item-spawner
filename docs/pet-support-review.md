# Pet and other equipment feasibility review

Historical feasibility snapshot. For the subsequent implementation and current live
results, see the [implementation and acceptance report](item-menu-implementation.md).

Reviewed 2026-10-12. This is a static feasibility review, not an implemented or
runtime-verified pet feature. No game files or saves were modified.

The broader [item-spawning research report](item-spawn-research.md) supersedes
this initial review's scope and catalog counts, with wiki research and a scan of
all 178 installed gameplay packages. The key corrections are incorporated below.

## Conclusion

Pet pickups are a strong candidate for extending the existing spawn route. The
game represents pets as subclasses of `HeroEquipment`, constructs equipment
using the selected template's actual class, and supplies pet-specific dropped
equipment templates. Accessories also use the equipment route. Neither category
is currently supported by this overlay.

Start with a small set of template-default pets, after validating the currently
installed executable. Randomization and custom pet stats require additional work;
removing the weapon/armor type check alone would be insufficient.

## Build identity and evidence limits

The installed Steam manifest now reports **build 25887270**. REA identified the
installed `DunDefGame.exe` as x86 PE with SHA-256
`2b43fef8ecf629d72b9a3fc032a65d6bf0298fd983301e97cdbfb45fd6314883`.
The overlay, attachment helper, and engine still require build 25809881's hash,
`4e4b03765c9e74415e27e36d952ebecd86a655ca27d8a46b5285279773e6d0f4`.
The installed executable is therefore unsupported independently of pet support.
Do not just replace the accepted hash: re-establish native bindings, thread
identity, and reflection contracts for the new executable.

Two installed packages retain their earlier hashes:

| Package | SHA-256 |
|---|---|
| `UDKGame.upk` | `5f532d3ecf74d85a74be3c6c36ca986a88da1573909ec8525474743479e1e824` |
| `Startup_INT.upk` | `00656c92c2ad74769210ad914137d93444773057f6d7a3130171e85d42b41568` |

Fresh decompression of both installed packages matched the retained logical
analysis bytes. The UELib input was also checked against the logical script
package with only the documented decompression-header edits. This supports
reuse of package evidence, not reuse of native addresses.

REA supplied executable identity. Its native analysis provider was unavailable
(Ghidra/IDA not configured; Hopper unsupported on Windows). Supplemental package
metadata/tag parsing and UELib script decompilation supplied the findings below.
UELib emitted trailing-data warnings and package deserialization errors; these
are static observations with bounded coverage, not proof of in-game behavior.
Analysis logs remain under ignored `target/pet-review/`; no decompiled game code
or assets are included in this document or tracked changes.

## Observed game support

- `HeroEquipmentNative.EEquipmentType` declares familiar/pet type **5** and
  accessory types **7, 8, 9, 10**. `EQT_ENDPRIMARYITEMS` at **6** is also used by
  the consumable class default, despite the enum member's name. An extension
  should allow explicit validated categories rather than a broad numeric range.
- The expanded startup inheritance scan found **216 non-default archetypes
  derived from `HeroEquipment_Familiar`**, across 16 instantiated pet subclasses.
  The initial count of 203 missed 13 templates from seven CDT-specific subclasses.
  These are candidate references, not 216 tested or necessarily user-facing pets.
- None of those pet paths occurs in the current protocol allowlist. That list
  contains 858 references selected as exact `HeroEquipment` instances; the UI
  currently exposes 592 weapon/armor references.
- `DunDefHeroManager.ServerCreateDroppedEquipment` accepts `HeroEquipment`,
  selects the item's own drop template or a manager entry by equipment type, and
  uses the common equipment net-info route.
- `HeroEquipment.CreateEquipmentFromNetInfo` creates the template's actual class
  and invokes its initialization. This preserves a pet subclass rather than
  replacing it with a plain equipment object.

The following sampled archetypes explicitly declare `EQT_FAMILIAR`, a familiar
mesh, and a pet-specific `DroppedEquipmentTemplates` entry:

| Example | Exact object path | Startup export index |
|---|---|---:|
| Fairy | `DunDefEquipment_Familiars.Equipment_Familiar_Fairy` | 33449 |
| Giraffe | `DunDefEquipment_Familiars.Equipment_Familiar_Giraffe` | 33466 |
| Genie | `DunDefEquipment_Familiars.Equipment_Familiar_Genie` | 36577 |
| Apprentice guardian | `FamGuardians.EquipmentGuardianMage` | 36617 |
| Dragon | `DunDefEquipment_Familiars.Equipment_Familiar_Dragon` | 36677 |
| Seahorse | `Fam_Seahorse.Equipment_Familiar_SeahorseProjectile` | 36689 |

Export indices identify package records, not process addresses. Presence in a
startup package does not prove that every reference is usable in a live session.

Accessories are also promising: their templates use ordinary `HeroEquipment`
with accessory types and their own pickup templates. Classifying by filename
alone is incorrect: `DunDefAccessories.Base.AccessoryBaseEquipment`, for example,
declares the gloves type, while its mask derivative declares `EQT_MASK`.

## Other item categories

A follow-up inspection on 2026-10-12 found these additional equipment candidates
in the same verified startup package. Each sampled collectible below is a
`HeroEquipment` archetype with an explicit dropped-equipment template; no live
spawn, pickup, redemption, or save/reload was performed.

| Category | Observed examples | Static evidence |
|---|---|---|
| Accessories | Hats/brooches, bracers, shields, masks | Base derivatives map to types 7, 8, 9, 10 respectively |
| Collectible materials | Coal, Small Coal, Radioactive Coal, Mote Of Ember | Equipment archetypes; sampled material entries enable stacking |
| Egg items | Egg, Humongous Egg, Free-Range Organic Egg | Equipment archetypes with egg pickup templates |
| Inventory mana tokens | Mana Token, Pirate Mana Token | Equipment archetypes with token pickup templates |

Representative source records are
`Coal_and_Diamond_Pets.Archetypes.Coal_Equipment` (export 35739),
`DunDefEquipment.ManaToken.ManaToken_Equipment` (36328),
`DunDefFireArena.EmberToken.EmberToken_Equipment` (36394), and
`EasterEggs.EasterEggSmall_Equipment` (36399).

All sampled material, egg, and inventory-token items use `EQT_FAMILIAR` (5),
despite being ordinary `HeroEquipment` rather than a familiar subclass. The
equipment type alone therefore cannot distinguish actual pets from collectibles.
Use explicit template/class metadata for the UI and stat editor. Default-template
pickup creation is a plausible extension; stacking, item value, and any exchange
or redemption behavior require their own checks.

World mana pickups are a separate case: `DunDefManaToken` derives from
`Engine.KActorSpawnable`, exposes `ManaPower`, and has its own collection/lifetime
handling. It is not the inventory `ManaToken_Equipment` archetype and cannot be
passed as `HeroEquipment` to the current adapter's spawn route.

`HeroEquipment_Consumable` and `HeroEquipment_Rune` exist as script classes, but
the expanded scan found no non-default equipment instances from this class family
across all 178 gameplay packages. Only class defaults were present. Consumable
drag/use and rune-handling code remains, but the attempted rune tier-method
decompilation emitted a control-flow formatting error. Usable rune/consumable
templates and their grant/use lifecycle remain unverified; class names alone do
not establish support. See the broader report for all 47 confirmed collectible
templates and separate world-pickup systems.

## Required changes and pet-specific pitfalls

1. **Catalog and UI:** include verified pet subclass archetypes, add a Pet
   category, and replace the weapon-versus-armor boolean editor with explicit
   supported categories. Unknown paths currently receive weapon controls.
   See `crates/overlay/src/catalog.rs`, `main.rs`, and `editor.rs`.
2. **Adapter admission:** `crates/adapter/src/engine.rs` currently requires a
   catalog path and rejects `EquipmentType > 4`. Retain the archetype, fresh-ID,
   saveability, loaded-reference, pickup-template, and local-session checks.
   The existing object cache already recognizes equipment through inheritance.
3. **Template defaults first:** the existing protocol already carries a path,
   quantity, and category-neutral template-default options. This initial mode
   needs no new command or pet actor spawning API. Let normal game pickup and
   equip handling establish the pet's behavior.
4. **Subclass-aware randomization:** the filtered/tavern roll path in
   `crates/adapter/src/engine/equipment.rs` resolves the base
   `HeroEquipment.AddRandomizeValues` function directly. Pet overrides add
   behavior: the familiar base randomizes another value, projectile pets apply
   an attack-rate cap, and guardians adjust boost randomization. Resolve and
   validate the appropriate override before enabling these modes for pets.
5. **Pet-specific custom fields:** the current adapter admits weapon overrides
   only for type 0 and resistance overrides for all nonzero types. Pets reuse
   some weapon fields for damage, healing, mana, or boosting; the meaning depends
   on the subclass. Guardian `InitFromNetInfo` also clamps or clears fields based
   on its boost type. Verify per-family labels, bounds, serialization, and final
   read-back instead of exposing the entire weapon editor unchanged.

## Validation still required

After establishing support for the new executable, test representative damage,
healing, mana, guardian, and stat-only pets in local single-player. Confirm the
pickup's class/archetype and fresh IDs, normal collection into inventory, normal
equipping and pet behavior, unequipping, and save/reload persistence. Only an
observed floor actor justifies a pickup-spawn confirmation; a successful grant
requires inventory confirmation. Unverified categories/modes must reject requests.

Run the repository's formatting, Clippy, workspace tests, and 32-bit adapter smoke
test when implementing support, followed by the live checks above. No Rust code
changed in this review, and no pet was spawned or granted.
