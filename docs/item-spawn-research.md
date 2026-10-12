# Dungeon Defenders item-spawning research

Historical research snapshot. For the subsequent implementation and current live
results, see the [implementation and acceptance report](item-menu-implementation.md).

Reviewed 2026-10-12 against the installed original Dungeon Defenders PC game and
overlay revision `130b4dc`. Wiki research preceded the expanded package scan.
This is a research report: no spawning feature was implemented, and no game
process, installation file, or save was modified.

## Findings

**Weapons, armor, and pets are not the complete inventory-item list.** Accessories,
forge resources, eggs, pet-exchange tokens, Experience Orbs, and inventory mana
tokens also have concrete equipment templates in this installation. These are
strong candidates for extending the overlay's existing equipment-pickup route.
Static presence does not establish that every template can be spawned, collected,
redeemed, and saved successfully in the running game.

The verified `Startup_INT.upk` inventory contains **1,075 non-default equipment
archetypes**:

| Family | Templates | Evidence and interpretation |
|---|---:|---|
| Weapons and armor | 592 | Exact path-set match with the current overlay UI catalog |
| Pets | 216 | Descendants of `HeroEquipment_Familiar`, across 16 instantiated classes |
| Accessories | 220 | 66 hat/brooch, 52 bracer, 32 shield, and 70 mask templates by inherited equipment type |
| Non-equippable collectibles | 47 | Ordinary `HeroEquipment`; inherited `bCanBeEquipped=false`; each has a concrete dropped-equipment template reference |

These are template counts, including base/test/event variants, not counts of
distinct released rewards or live-tested spawnable items. For example, a pet
template explicitly named `Equipment_Familiar_SheepPet_TigiTest` is present.

## Wiki findings checked against installed files

The wiki's main [Equipment page](https://dungeondefenders.wiki.gg/wiki/Equipment)
describes the four equipped families. Its
[Coal Forge](https://dungeondefenders.wiki.gg/wiki/Coal_Forge) and
[Pet Forge](https://dungeondefenders.wiki.gg/wiki/Pet_Forge) pages expose additional
inventory resources and exchange tokens that the basic equipment classification
does not cover. The table below uses those pages as leads and installed exports
as the evidence for template availability.

| Wiki family | Confirmation in this installation | Feasibility for this overlay |
|---|---|---|
| [Pets](https://dungeondefenders.wiki.gg/wiki/Equipment) | 216 familiar-subclass archetypes; examples include Fairy, Genie, Seahorse, guardians, healing pets, and CDT-specific pets | Strong equipment-route candidate; subclass behavior must be preserved |
| [Accessories](https://dungeondefenders.wiki.gg/wiki/Accessories) | Equipment types 7–10; four slots, with hats grouped with brooches | Strong equipment-route candidate; explicit accessory controls needed |
| [Coal and forge resources](https://dungeondefenders.wiki.gg/wiki/Coal_Forge) | Coal, Small Coal, Radioactive Coal, Heart Fragment, Mote Of Ember: **5 templates** | Equipment pickup candidates; exchange recipes and effects require separate validation |
| [Historical eggs](https://dungeondefenders.wiki.gg/wiki/Egg_%28Easter%29) and [Organic Egg](https://dungeondefenders.wiki.gg/wiki/Pet_Forge) | Egg, Large, Big, Huge, Gigantic, Humongous, and Free-Range Organic Egg: **7 templates** | Equipment pickup candidates; an old egg remaining in files does not prove its old event is active |
| [Pet-exchange tokens](https://dungeondefenders.wiki.gg/wiki/Pet_Forge) | **32 templates**: 3 assault horns, 10 Returnia gems, 15 Warping Core II Zamira tokens, 4 original Warping Core Zamira tokens | Equipment pickup candidates; pet redemption is a distinct game operation |
| [Experience Orb](https://dungeondefenders.wiki.gg/wiki/Experience_Orb) | **1 template**, `DunDef_CDT_Update7.BossRush.EXP_Equipment` | Physical item confirmed; XP/redemption effects were not exercised |
| [Mana Token](https://dungeondefenders.wiki.gg/wiki/Mana_Token) and [Pirate Mana Token](https://dungeondefenders.wiki.gg/wiki/Pirate_Invasion) | **2 inventory templates** with dropped-equipment references | Equipment pickup candidates; stored mana and value initialization matter |

The last five rows account for all **47** startup collectibles. Appendix A lists
every path and export index. They all use equipment type `EQT_FAMILIAR` (5), but
none is a familiar subclass. Consequently, **type 5 must not be labeled “pet”
without checking class and template metadata**. Likewise, an item described as a
consumable by players need not use the game's `HeroEquipment_Consumable` class:
the Experience Orb is ordinary `HeroEquipment` with equipping disabled.

The [Event Items](https://dungeondefenders.wiki.gg/wiki/Event_Items) page describes
an origin/reward distinction spanning existing equipment families. “Event item”
is not an additional universal engine type. Colored egg-shaped event pets also
must not be confused with the older collectible eggs.

### Other objects called items

| Family | Wiki lead and local evidence | Relationship to equipment spawning |
|---|---|---|
| Loose mana gems | [Mana](https://dungeondefenders.wiki.gg/wiki/Mana); concrete `DunDefArchetypes.*ManaToken*` exports use `UDKGame.DunDefManaToken`, derived from `Engine.KActorSpawnable` | Separate collectible actor and mana-collection route; not an inventory Mana Token |
| Fruit buffs | [Beach Island](https://dungeondefenders.wiki.gg/wiki/Beach_Island); five `CDT_BeachIsland.BeachIsland_Bufftoken_1` through `_5` templates use `DunDefBuffToken` with damage, gravity, healing, shield, and speed buff references | Separate temporary-effect actors, not item-box equipment |
| Golden mission tokens | [Treasure Hunt](https://dungeondefenders.wiki.gg/wiki/Treasure_Hunt); `DunDefArchetypes.ManaToken_Golden` uses `DunDefSpecial.DunDefManaToken_Golden` | Mission-aware mana-token subclass; requires its own spawn and objective handling |
| Presents | [Etherian Holiday Extravaganza](https://dungeondefenders.wiki.gg/wiki/Etherian_Holiday_Extravaganza) and [Silent Night](https://dungeondefenders.wiki.gg/wiki/Silent_Night); `ChristmasMission.Presents.ParentPresent` exists in `DD_Christmas.upk` and `DD_SilentNight.upk` | `DunDefPickupableItem` derived from `DunDefDroppedItem`; carry/delivery actor, not persistent equipment |
| Capture-the-flag flags | [PvP maps](https://dungeondefenders.wiki.gg/wiki/PvP_Maps); `CTF_MultiFlag.Flag.CTMF_Flag` and `DunDefCTF.Flag.CTF_Flag` occur in installed map packages | `DunDefSpecial.CTF_Flag` derives from `DunDefDroppedItem`; team/objective lifecycle is separate |
| Costumes | [Costumes](https://dungeondefenders.wiki.gg/wiki/Costume); `UnlockCostume` updates costume IDs in saved options; `IsCostumeUnlocked` checks those IDs | Unlock state, not a normal equipment template to grant |
| Crystal appearances | [Crystal Skins](https://dungeondefenders.wiki.gg/wiki/Crystal_Skins); crystal customization uses `CrystalCoreOptions` and achievement-based unlock checks | Appearance/options state, not ordinary inventory equipment |
| Other mission interactables | [War of the Djinn](https://dungeondefenders.wiki.gg/wiki/War_of_the_Djinn) lamps and [Rumble in the Jungle](https://dungeondefenders.wiki.gg/wiki/Rumble_in_the_Jungle) objectives | Wiki leads only in this review; their individual actor lifecycles were not traced |

These actor examples demonstrate that “all spawnable things” is wider than “all
inventory-item families.” The report does not claim an exhaustive catalog of
enemies, towers, projectiles, map props, or mission actors, nor that spawning one
of these actors outside its map would work correctly.

### Runes and generic consumables

The files contain `HeroEquipment_Consumable` and its `HeroEquipment_Rune`
subclass. The consumable class default sets `bCanBeEquipped=false`,
`bIsConsumable=true`, and equipment type **6** (`EQT_ENDPRIMARYITEMS`). Despite
that enum member's name, type 6 is therefore used by a real class default.

Script also contains an item-box drag-to-consume path and rune handling:
`UI_ItemBox.NotifyDraggedEvent` checks consumable acceptance, validates the
operation, invokes `HandleConsumables`, and removes consumed entries. However,
the full gameplay-package scan found **no non-default consumable/rune equipment
instances**, including descendants resolved through the discovered class graph.
Only class defaults remain. The wiki's
[PC patch notes](https://dungeondefenders.wiki.gg/wiki/Patch_Notes_%28PC%29)
describe removal of a leftover rune testing feature in 10.2.1, consistent with
retained code without a packaged usable-item catalog.

This does not prove it is impossible to manufacture an object of those classes.
It does mean there is insufficient evidence to offer runes or generic consumables
as supported spawnable items. A class default is not a validated item archetype.
The rune tier-method decompilation also had a control-flow formatting failure;
that output was not treated as reliable behavior evidence.

Redux-specific [Divine Crystals](https://dungeondefenders.wiki.gg/wiki/Divine_Crystal)
and [Void Crystals](https://dungeondefenders.wiki.gg/wiki/Void_Crystal),
[Eternity's](https://dungeondefenders.wiki.gg/wiki/Dungeon_Defenders_Eternity)
potions/dyes, and
[Awakened's](https://dungeondefenders.wiki.gg/wiki/Dungeon_Defenders%3A_Awakened)
rune system must not be imported into this game's
supported catalog merely because they appear on the same wiki. Those are separate
edition/game systems. The inspected target is the installed original PC game.

## What the game code establishes

The static evidence supports an extension of the existing equipment route:

1. `DunDefHeroManager.ServerCreateDroppedEquipment` accepts `HeroEquipment`,
   selects its own dropped-equipment template or the manager's equipment-type
   fallback, and initializes the pickup through equipment net-info.
2. `HeroEquipment.CreateEquipmentFromNetInfo` creates the selected template's
   actual class and calls its initialization. Pet subclass identity can therefore
   survive the common equipment path.
3. Equipping and storage are different conditions. `AllowStoringInItemBox` checks
   `bCantSave` and `bCantPutInItemBox`; being non-equippable does not itself reject
   an item from storage.
4. The item-box path merges stackable items by base archetype. Grant verification
   must observe an inventory addition **or a correctly matched stack increase**.
   A new floor actor alone does not confirm a grant.
5. `GiveManaToken` initializes `StoredMana` and `MinimumSellWorth` through a token
   archetype before adding it to inventory. Merely selecting the token template
   does not establish a requested denomination.

Pet randomization needs additional care. The overlay currently resolves the base
`UDKGame.HeroEquipment.AddRandomizeValues` directly. Familiar subclasses add
randomization, projectile-rate limits, or guardian adjustments. Guardian
initialization also clamps/clears fields according to boost type. Template-default
spawning is the smallest promising initial mode; arbitrary weapon-style custom
stats and random rolls are not automatically valid for every pet family.

The Experience Orb's non-equippable template does not establish that the generic
first-time-equipped XP function handles it. Its specific redemption effect
remains unverified, as do forge exchange quantities and reward generation.

## Overlay implications and current blocker

The present UI lists 592 weapon/armor references. The protocol allowlist contains
858 exact-base-class equipment references, but the adapter explicitly admits
only equipment types 0–4. See `crates/overlay/src/catalog.rs`,
`crates/protocol/src/lib.rs`, and `crates/adapter/src/engine.rs:503` and `:537`.
The equipment cache already follows class inheritance; the main gaps are catalog
selection, category-specific validation, UI, and lifecycle verification.

The editor currently uses a weapon/armor boolean
(`crates/overlay/src/editor.rs:55`). Custom weapon overrides require type 0, while
resistance overrides are allowed for nonzero types
(`crates/adapter/src/engine/equipment.rs:199`). These rules cannot be applied to
all new categories by just raising the numeric type limit.

**The installed executable is currently unsupported by the overlay.** The Steam
manifest reports build **25887270**, while the source still pins the executable
for build **25809881**. The identity gate appears in `overlay/src/game.rs:9`,
`adapter/src/attachment.rs:36`, and `adapter/src/engine.rs:18` under `crates/`.

| Artifact | SHA-256 |
|---|---|
| Installed `Binaries/Win32/DunDefGame.exe` | `2b43fef8ecf629d72b9a3fc032a65d6bf0298fd983301e97cdbfb45fd6314883` |
| Executable accepted by overlay | `4e4b03765c9e74415e27e36d952ebecd86a655ca27d8a46b5285279773e6d0f4` |
| Installed `UDKGame.upk` | `5f532d3ecf74d85a74be3c6c36ca986a88da1573909ec8525474743479e1e824` |
| Installed `Startup_INT.upk` | `00656c92c2ad74769210ad914137d93444773057f6d7a3130171e85d42b41568` |

Do not just replace the accepted hash. Native bindings, execution-thread checks,
and reflection/layout contracts must first be established for this executable.
Unchanged script packages do not prove unchanged native addresses.

A reasonable implementation order is: validate the executable integration; add a
small reviewed set of template-default pets, accessories, and collectibles; then
validate each family's optional customization and redemption behavior. Preserve
loaded-reference, archetype, saveability, pickup-template, fresh-ID, and supported
session checks. Unknown templates or modes must continue to reject requests.

For each admitted family, validate floor pickup, inventory/stack confirmation,
appropriate equip or use behavior, and save/reload persistence in a local test
session. Pets additionally need observed behavior after equipping. Run repository
formatting, Clippy, workspace tests, and the 32-bit adapter smoke test when code
changes are implemented. This documentation-only investigation did not run those
Rust checks or any live grant tests.

## Scope, method, and uncertainty

- **Target:** `D:\SteamLibrary\steamapps\common\Dungeon Defenders`; all **178**
  `.upk` files under its `UDKGame` directory, totaling **9,700,476,197 bytes**.
  The game's 20 additional Engine editor/support packages were outside this
  gameplay inventory. Workshop/mod installations and future builds are outside
  scope.
- **Coverage:** name, import, export, and class-inheritance tables across all 178
  packages; selected equipment property tags and archetype inheritance. Supported
  observed formats were 678/2 (18 packages) and 679/2 (160 packages). Final scan:
  **zero package parse errors and zero equipment-property parse errors**.
- **Counts:** 5,590 equipment-derived export records, including defaults and
  cooked/localized copies; **1,184 distinct non-default archetype `(class, path)`
  pairs** across packages. These are candidates, not 1,184 tested items.
  Same-path copies may differ by package. Counts for the main family breakdown
  deliberately use the single verified startup package.
- **Completeness check:** the final class graph's equipment-descendant export
  counts matched all 5,590 selected records. This checks extraction coverage for
  the discovered equipment hierarchy; it does not prove every possible dynamic
  item or actor is represented by an exported archetype.
- **Tools:** REA established x86 PE executable identity. Its native providers
  were unavailable, so supplemental read-only UE3 metadata/property parsing and
  UELib selected-method decompilation supplied the package findings. UELib emitted
  trailing-data warnings and unrelated deserialization errors. Static code
  interpretations remain distinct from runtime/native validation.
- **Prior evidence reuse:** freshly decompressed installed script/startup bytes
  matched retained logical analysis bytes. The UELib input matched the logical
  script package apart from documented decompression-header changes. Native
  addresses were not carried forward from another build.
- **Wiki access:** direct page requests returned HTTP 403; search-index text for
  the linked wiki pages supplied the research leads. It is a potentially stale
  snapshot, not a guarantee that every live page revision was read. Local package
  evidence takes precedence for this installation.
- **Observed wiki drift:** the Pirate Invasion page describes unstackable pirate
  tokens, but the installed Pirate Mana Token template explicitly sets stacking
  true. Forge pages also mix historical and current behavior. This report does
  not infer active recipes solely from old wiki descriptions.

Local analysis artifacts are in ignored `target/item-research/`:
`scan_packages.py`, `package-inventory.json` (per-package hashes and records),
`research-summary.json`, `world-pickups.json`, `mission-pickups.json`, and
`script-review/`. Earlier pet-specific evidence is in `target/pet-review/`.
Game assets, decompiled source, and analysis toolchains remain outside Git.

This supersedes the earlier count of 203 pets: seven CDT-specific subclasses add
13 templates, producing 216 across 16 classes. It also corrects the earlier
description of type 6 as an unused separator and expands the rune/consumable
search from startup-only to all 178 gameplay packages.

## Appendix A: all 47 startup collectibles

Names below preserve the installed display strings, including spelling mistakes.
Every row is an ordinary `UDKGame.HeroEquipment` archetype with type 5,
`bCanBeEquipped=false`, and a resolved nonempty dropped-equipment template
reference. Export indices are one-based package records, **not process addresses**.
This table proves packaged references; it does not certify runtime usability or
redemption. Stackability must be resolved per template; missing serialized tags
must not be interpreted as an explicit false value.

| Item | Startup export | Exact object path |
|---|---:|---|
| Humongous Egg | 33464 | `EasterEggs.EasterEggBig_Equipment_Nightmare` |
| Coal | 35739 | `Coal_and_Diamond_Pets.Archetypes.Coal_Equipment` |
| Small Coal | 35740 | `Coal_and_Diamond_Pets.Archetypes.Coal_Equipment_Small` |
| Unicorn Horn of the Forest | 35801 | `DunDef_CDT_Forge.Archetypes.HornPack1_Equipment` |
| Rainbow Horn of the Tree | 35802 | `DunDef_CDT_Forge.Archetypes.HornPack2_Equipment` |
| Golden Horn of the Mines | 35803 | `DunDef_CDT_Forge.Archetypes.HornPack3_Equipment` |
| Free-Range Organic Egg | 35841 | `DunDef_CDT_PayloadDefense.OrganicMegaChicken.OrganicEasterEgg_Equipment` |
| Kraken Gem | 35842 | `DunDef_CDT_Returnia.Forge.Archetypes.Aquanos.Aquanos_Shard_Equipment` |
| Small Kraken Gem | 35843 | `DunDef_CDT_Returnia.Forge.Archetypes.Aquanos.Aquanos_Shard_Equipment_Small` |
| Old One's Gem | 35844 | `DunDef_CDT_Returnia.Forge.Archetypes.CD.CD_Shard_Equipment` |
| Small Old One's Gem | 35845 | `DunDef_CDT_Returnia.Forge.Archetypes.CD.CD_Shard_Equipment_Small` |
| Spider Queen Gem | 35846 | `DunDef_CDT_Returnia.Forge.Archetypes.Misty.Misty_Shard_Equipment` |
| Small Spider Queen Gem | 35847 | `DunDef_CDT_Returnia.Forge.Archetypes.Misty.Misty_Shard_Equipment_Small` |
| Genie King's Gem | 35848 | `DunDef_CDT_Returnia.Forge.Archetypes.Moraggo.Moraggo_Shard_Equipment` |
| Small Genie King's Gem | 35849 | `DunDef_CDT_Returnia.Forge.Archetypes.Moraggo.Moraggo_Shard_Equipment_Small` |
| Battlecruiser Gem | 35850 | `DunDef_CDT_Returnia.Forge.Archetypes.SkyCity.SkyCity_Shard_Equipment` |
| Small Battlecruiser Gem | 35851 | `DunDef_CDT_Returnia.Forge.Archetypes.SkyCity.SkyCity_Shard_Equipment_Small` |
| Heart Fragment | 35980 | `DunDef_CDT_SkyOLove.Equipment.HeartFragment.HeartFragment_Equipment` |
| Experience Orb | 36063 | `DunDef_CDT_Update7.BossRush.EXP_Equipment` |
| Pirate Mana Token | 36118 | `DunDef_CDT_Update9.ManaToken.Pirate_Mana_Equipment` |
| Zamira's Gift of Knowledge | 36129 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Arcane` |
| Zamira's Lesser Knowledge | 36130 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Arcane_hard` |
| Zamira's Knowledge | 36131 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Arcane_insane` |
| Zamira's Ignornace | 36132 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Arcane_Medium` |
| Zamira's Greater Gift of Knowledge | 36133 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Arcane_Ruthless` |
| Zamira's Gift of Patience | 36134 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Infested` |
| Zamira's Lesser Patience | 36135 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Infested_hard` |
| Zamira's Patience | 36136 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Infested_insane` |
| Zamira's Impatience | 36137 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Infested_Medium` |
| Zamira's Greater Gift of Patience | 36138 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Infested_Ruthless` |
| Zamira's Gift of Resilience | 36139 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Omenak` |
| Zamira's Lack Of Resilience | 36140 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Omenak_hard` |
| Zamira's Resilience | 36141 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Omenak_insane` |
| Zamira's Weakness | 36142 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Omenak_Medium` |
| Zamira's Greater Gift of Resilience | 36143 | `DunDef_CDT_WarpingCore.Forge.ZamirasGift.ZamirasGift_Equipment_Omenak_Ruthless` |
| Zamira's Gift: Castle Armory | 36145 | `DunDef_CDT_WarpingCore.OGWarpingCore.Forge.ZamirasGift_Equipment_Armory` |
| Zamira's Gift: Hall of Court | 36146 | `DunDef_CDT_WarpingCore.OGWarpingCore.Forge.ZamirasGift_Equipment_Court` |
| Zamira's Gift: Royal Gardens | 36147 | `DunDef_CDT_WarpingCore.OGWarpingCore.Forge.ZamirasGift_Equipment_Garden` |
| Zamira's Gift: Endless Spires | 36148 | `DunDef_CDT_WarpingCore.OGWarpingCore.Forge.ZamirasGift_Equipment_Spire` |
| Radioactive Coal | 36152 | `DunDef_CDT_WinterWonderland2.CoalPet.Coal_Radioactive_Equipment` |
| Mana Token | 36328 | `DunDefEquipment.ManaToken.ManaToken_Equipment` |
| Mote Of Ember | 36394 | `DunDefFireArena.EmberToken.EmberToken_Equipment` |
| Large Egg | 36395 | `EasterEggs.EasterEggBig_Easy` |
| Huge Egg | 36396 | `EasterEggs.EasterEggBig_Hard` |
| Gigantic Egg | 36397 | `EasterEggs.EasterEggBig_Insane` |
| Big Egg | 36398 | `EasterEggs.EasterEggBig_Medium` |
| Egg | 36399 | `EasterEggs.EasterEggSmall_Equipment` |
