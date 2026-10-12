# Item menu expansion — implementation and acceptance

Historical protocol-v5 acceptance snapshot. The [automatic catalog report](automatic-catalog.md)
describes the subsequent protocol-v6 scanner and cache implementation.

Updated 2026-10-12. Target: original Steam PC build **25887270**, one local
single-player hero. Protocol **v5**. This report separates implemented routes,
static evidence, automated checks, and outstanding live acceptance.

## Implemented behavior

One bundled metadata catalog drives the menu and adapter. All six categories
have name/path search and family or slot filters. The original weapon/armor
filter assignments remain intact. Excluded entries remain visible with reasons.
The selected item is inspected in the running game before the button is enabled;
changing selection or reconnecting invalidates that inspection. The adapter
rechecks catalog membership, options, ownership, reference identity, template
class, saveability, and session independently before each spawn.

Equipment supports defaults, natural random rolls, and checked custom values.
Pet ability controls follow reviewed class semantics; accessories expose only
applicable resistance controls. Collectibles accept quantity and, for inventory
mana tokens, an optional denomination. World actors use a separate typed route.
All requests retain the 1–10 pickup limit. Unknown manual paths are rejected.

Custom equipment is reconstructed with its original archetype and actual
subclass, initialized by the game, and verified after floor-pickup creation.
IDs must be nonzero and distinct in a batch. Guardian boost storage follows
the template's boost enum; invalid subclass initialization cannot silently
change a requested value. Inventory mana tokens store the denomination in the
low component of HighDigitInt, with the high component zero, for values up to
2,000,000,000. Redemption is left to the game.

Missing catalog templates use checked game loading APIs. Source filenames and
SHA-256 hashes are bundled metadata; source files must resolve inside the
installation's UDKGame directory. References are refreshed after package loads
and world changes. Reentrant engine requests are rejected during loader message
pumping. Disconnect or a changed map/hero stops further spawning.

Successful responses identify confirmed floor actors, not inventory acquisition.
Failed batches carry their confirmed count and uncertainty. The existing paced
search and cancellation behavior is retained, with no automatic retry after an
ambiguous dispatched operation. Framing remains bounded to 16 KiB. Only a
complete, valid current-version spawn receives the 30-second loading response
budget; incomplete messages still expire within two seconds.

## Catalog accounting

The 178-package scan accounts for every **1,184** unique equipment candidate,
including **1,075** startup templates. Cooked/localized copies are deduplicated
by class and exported identity; distinct variants are retained.

| Category | All packages | Startup templates |
|---|---:|---:|
| Weapons | 575 | 562 |
| Armor | 66 | 30 |
| Pets | 275 | 216 |
| Accessories | 221 | 220 |
| Collectibles | 47 | 47 |
| Total equipment | 1,184 | 1,075 |

The catalog additionally records 17 world-pickup exports: 15 reusable qualified
templates, one placed flag instance, and one unqualified flag export. There are
**52 static exclusions** overall: 44 equipment candidates forbid saving/storage,
six equipment identities and one flag identity are unqualified, and one flag is
a placed level instance. These records are accounted for rather than silently
discarded. Test-named variants are tagged, not excluded solely by name.

[Catalog metadata and provenance](../crates/protocol/README.md) describe the
reproducible generator, package hashes, inheritance resolution, and coverage
tests. The metadata contains no game assets or decompiled code.

## Family support matrix

“Implemented” below describes the checked code route. Representative defaults
and custom values passed for all 16 pet subgroups and all four accessory slots.
All 20 Tavern-testing random representatives passed after correcting one
authored-sentinel guard. Inventory/save-reload passed for the 69 first-session
equipment pickups. Complete mission and active-behavior acceptance remains unfinished.
A selectable catalog
candidate is not proof that its package, initialization, collection, or save
lifecycle works. Static exclusions and live inspection further restrict every
row. Counts include excluded candidates.

| Family | Count | Implementation status and evidence |
|---|---:|---|
| Weapons / armor | 575 / 66 | Implemented existing equipment route; current-build default crossbow/chain-boots pickups passed in Tower Wars; older detailed results belong to build 25809881 |
| Projectile pets | 118 | Implemented; reviewed exact classes expose damage/rate and applicable projectile fields |
| Melee pets | 86 | Implemented; reviewed exact classes expose damage/rate |
| Player-healing pets | 11 | Implemented; healing maps to the family's serialized damage field |
| Tower-healing pets | 16 | Implemented; healing mapping reviewed in subclass code |
| Mana-generation pets | 1 | Implemented; mana-generation mapping reviewed in subclass code |
| Guardians | 21 | Implemented; boost enum selects the field preserved by subclass initialization; range uses clip-ammo storage |
| Hero boosting | 6 | Implemented common equipment controls; no unverified ability overrides |
| Buff spawning | 2 | Implemented common equipment controls; no unverified ability overrides |
| Area boosting | 2 | Implemented common equipment controls; no unverified ability overrides |
| Ally summoning | 1 | Implemented common equipment controls; no unverified ability overrides |
| Sheep | 2 | Implemented common equipment controls; no unverified ability overrides |
| Target damage | 1 | Implemented common equipment controls; no unverified ability overrides |
| Tower-scaled melee / damage | 3 / 1 | Implemented common equipment controls; no unverified ability overrides |
| Crystal healing / mini queen | 2 / 2 | Implemented common equipment controls; no unverified ability overrides |
| Hat/brooch / bracers | 66 / 53 | Implemented accessory route, slot classification, capability-gated resistances |
| Mask / shield | 70 / 32 | Implemented accessory route, slot classification, capability-gated resistances |
| Resources | 5 | Implemented; coal collected as separate inventory entries and merged into a stack on save/reload |
| Eggs | 7 | Implemented template-preserving collectible route; exchange lifecycle pending |
| Exchange tokens | 32 | Implemented template-preserving collectible route; redemption pending |
| Experience Orbs | 1 | Implemented template-preserving collectible route; experience redemption pending |
| Inventory mana tokens | 2 | Implemented default/custom denomination with matching value initialization and readback |
| Loose mana | 6 | Implemented physical actor route; requires normal Main and DD replication-info context |
| Fruit buffs | 5 | Implemented physical actor route; verifies buff-template dependency |
| Golden mission tokens | 1 | Context-dependent: requires GoldenTokens replication info; collection calls its mission callback |
| Presents | 1 | Context-dependent: requires Delivery replication info and player controller; initializes and checks the chosen mesh |
| Flags | 4 | Blocked: CTF team/controller/base initialization is not established; also includes the placed and unqualified exclusions |
| Runes | 0 usable templates | Blocked: consumable class/default evidence alone does not establish concrete usable items |

Actual pets are classified by familiar inheritance. The 47 collectibles also
use equipment type 5 but do not inherit familiar behavior. Costumes and crystal
skins remain unlock systems. Enemies, towers, projectiles, and general props
are outside this menu.

Mission pickups are **not generally available in the tavern**. Golden-token
collection directly calls a mission-specific replication-info method. Present
pickup/drop calls delivery-controller methods. Flag startup and reset require
CTF replication info and a flag base. The implementation reports these concrete
limits instead of fabricating controllers or mission progress. The request for
outside-map use remains blocked for those families until their entire lifecycle
is established; a mesh appearing by itself would not satisfy acceptance.

## Current-build integration evidence

The executable hash, object/name tables, game-thread globals, dispatcher ABIs,
and loader entry point were recovered from the current installed executable.
Matching script-package identities corroborate reflection contracts. The
adapter checks reflection types, dimensions, masks, function flags, parameter
offsets, class constraints, dispatcher targets, and local ownership at runtime.
Unknown builds and unsupported sessions reject requests.

See [current-build validation](current-build-validation.md) for addresses,
ownership analysis, observed loader flags, evidence locations, and the read-only
runtime probe. Its static evidence does not replace live execution acceptance.

## Automated verification

- Workspace Clippy with `--all-targets -- -D warnings`: passed.
- 32-bit adapter Clippy with all targets and warnings denied: passed.
- `cargo test --workspace`: passed, 20 adapter + 16 overlay + 23 protocol tests.
- `cargo fmt --all --check` and `git diff --check`: passed.
- Packaged x86 smoke and transport tests: passed, including incomplete v5 spawn
  framing, version mismatch, inspection, and diagnostic spawn rejection.

Tests cover catalog counts/deduplication/classification and source hashes,
legacy filters, names/paths, type-5 collectibles versus pets, capability and
category rejection, editor resets, serialization bounds, partial results,
stale inspection replies, cancellation, attempt limits, uncertainty, protocol
versions, framing, and game-thread queue expiry. Unit tests do not simulate a
working engine or prove native dispatch safety.

The initial sandboxed workspace link failed while replacing a generated DLL
import library (LNK1114, access denied). The same full test command passed with
permission to replace that generated artifact. No source change was used to
hide the failure.

## Live observations

Read-only validation in local tavern process 12704 confirmed the current image,
game/window thread 9656, both dispatcher targets, exact function parameters,
118 equipment properties, 27 session fields, and the 540/412-byte structures.
The attached adapter then reported a ready standalone local session.

An exhaustive inspection covered all **1,201 catalog records** in that tavern:
1,086 loaded entries passed, 55 were potentially loadable, and 60 were
unavailable. The 60 comprise 52 catalog exclusions, four parent-forcing pet
templates, two reusable flags, one golden token, and one present. “Potentially
loadable” is deliberately not a successful load or spawn claim.

Initial live batches confirmed **71 pickups**: 6 in the first mixed batch, 26
family/default cases, 20 custom cases, and 19 random cases. Defaults and custom
readback covered every pet subgroup and accessory slot. Five collectible
families, loose mana, and fruit spawning were represented. A fruit-spawning pet
random case safely rejected the authored -1 promotion threshold; current game
code compares FRand directly with that threshold, so the guard was corrected
to preserve it. After restart, the corrected adapter successfully randomized
and spawned that pet. Custom Mage, Monk, and Squire guardian boost/range cases
also passed, extending coverage beyond the initial Huntress guardian mapping.

The user collected and equipped items normally. Read-only comparison confirms
all **34 test identities present in the intermediate floor snapshot** moved to
the item box with matching IDs, subclasses, archetypes, and values. Inventory
grew from 46 to 114 entries. The combined inventory/equipped difference contains
69 new identities across the exact 25 expected equipment/collectible templates,
matching all 69 confirmed equipment pickups. A Summoner pet with a 251-health bonus was
equipped. The initial custom bracer retained 250 health and all four 10-point
resistances; the mana token retained MinimumSellWorth=100000, StoredMana=0.
No accessory was equipped in that snapshot, despite successful collection.
The active level-8 Hermit could not use the level-100 Glass accessories; a
basic Bracer with no level/class restriction was spawned for a separate check.
That basic accessory was subsequently confirmed in both the hero's equipped
array and the item's hero-ownership field. Its actual generated display name
was **Torn Buckled Wristguards Lvl 1**; “Bracer” is the template's catalog name.
This establishes representative accessory equipping, not every slot/class pairing.

Three coal pickups remained three separate inventory entries with counter zero
(one item each). This matches `TransferEquipmentToItemBox`, which reconstructs
and directly appends the pickup. Reward/give-route `AddEquipmentToItemBox` has a
different stack-merge branch. Preserving normal pickup does not authorize
silently replacing it with the reward route. On normal game exit and restart,
the game's save-info import **merged those three coal entries into one stack of
three**, confirmed by the raw counter changing from 0 to 2. The two removed
identities are accounted for by that merge, not treated as lost items.

The read-only post-restart snapshot in process 39496, taken before new spawns,
confirmed 112 item-box entries and four equipped items. All compared quantities
and values survived, including all 69 acquired equipment pickups, their actual
subclasses/archetypes, the 100000 and 123456 mana tokens, custom resistances,
and the equipped Summoner pet's 251 health bonus. No unexpected changes to the
pre-existing items were observed.

The user then loaded **Tower Wars** in the same process. Read-only comparison
confirmed all 116 item-box entries and five equipped items retained their IDs,
quantities, and checked values, including the basic wristguards. The map uses
`TW_GameInfo`, `CTD_DDGRI`, and `CTF_DDPC`; it is not the golden-token challenge.
Reconnecting the adapter in this map passed session validation, bracer/fruit
inspection, and explicit mission-item unavailability checks. A default crossbow
and chain-boots pickup also spawned successfully on the current build. These
two later pickups have not been checked for acquisition. The adapter was
disconnected during travel, so this verifies reconnection after travel rather
than continuous-attachment cache invalidation.

Loading the allowlisted Mephisto variant from an identity-checked LobbyLevel
source returned without resolving its catalog path; no pickup was dispatched.
The failure remains explicit. Subsequent inspection now retains a load failure
until a map change/reconnect or a resolved reference, instead of continuing to
advertise the same failed dependency as ready. The corrected adapter's rejection
and subsequent unavailable inspection both passed live. This does not establish loading
success for the other 54 missing candidates.

Mana/fruit actors were verified immediately after creation but absent at the
later snapshot; absence alone does not prove collection or buff activation.
Golden tokens, presents, and flags rejected tavern inspection with their
specific missing-lifecycle reasons; explicit spawn requests also rejected before
loading or dispatching their actors on the corrected adapter. No mission progress
was fabricated.

Ignored evidence lives under `target/item-implementation/`: current-runtime
validation/layout JSON, live inspection/spawn results, lifecycle snapshots and
comparison, and the read-only scripts. Only asset-free code, metadata, and reports
are intended for Git.

## Outstanding live acceptance

1. Verify every accessory slot/class pairing and each pet's actual active behavior;
   representative pet and bracer equipping passed, but creation/readback alone
   does not establish combat behavior.
2. Verify normal collectible redemption and meaningful value changes; physical
   collection, stack merging on reload, and representative persistence passed.
3. Test mana/fruit interaction, actor destruction, and cleanup. Exercise successful
   on-demand package loading and continuous-attachment map changes without stale
   references; reconnecting after the tavern-to-Tower-Wars transition passed.
4. Test golden tokens and presents in their missions, including cleanup and
   map transitions. Verify explicit outside-map rejection. Flags remain blocked.
5. Record individual candidate exceptions; representative successes must not be
   generalized to all 1,184 equipment candidates.

Until these checks run, this is a development preview with representative live
results and explicit blocked families, not a fully validated completion of the
entire acceptance plan.
