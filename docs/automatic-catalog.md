# Automatic catalog implementation and verification

Updated 2026-10-13. Protocol **v6** replaces the bundled item catalog with automatic
package discovery, caching, and preset rules. Native integration still targets
Steam build **25887270**.

## Behavior

The overlay scans the installed UDKGame package tree on a background worker.
Two passes establish complete class ancestry before extracting item properties.
The safe Rust reader supports installed 678/2 and 679/2 formats and bounded LZO
blocks. File sizes, logical data, tables, strings, counts, and cycles have explicit
limits. Unsupported formats and malformed records produce visible errors.

The six equipment/pickup categories and Unclassified are preset. Membership uses
inheritance, equipment type, resolved defaults, slots, and reviewed family rules.
Unfamiliar concrete classes stay unavailable even if their category is known.
General props, enemies, projectiles, and arbitrary actors are not item candidates.

Metadata is cached under `%LOCALAPPDATA%/DungeonWorkshop/cache`. Changed files
are reparsed; class-graph changes reconsider candidates across packages. Rebuild
forces a clean read. Rule updates reclassify cached facts; scanner/schema updates
rebuild them. Content-addressed generations and an atomic manifest preserve the
previous cache on cancellation/failure. Progress includes reused/reparsed counts.
Unknown entries, scan issues, stale completions, and incomplete catalogs never
enable spawning. Publishing a catalog requires reconnecting and fresh inspection.

Both sides bind the same generation under protocol v6. The pipe carries small
identifiers and typed requests, never the full catalog. Cache reads and package
hashing run on the adapter worker. Selected-source inspection reads the actual
export independently of cached classification, using a streamed reader that
avoids loading the entire package in the 32-bit game. The game thread checks actual
class/category, capability flags, consumable restrictions, dropped templates,
ownership, and lifecycle. Catalog data alone does not authorize native calls.

A small integration fingerprint list covers the executable, UDKGame.upk, and the
five localized startup packages containing reviewed scripts. This is separate
from the discovered item list. Changed integration scripts require validation;
scanning does not imply compatibility with arbitrary updates or mod behavior.

## Shared weapon dependency validation (scanner v3)

The item-specific `fenghuang.rs` validator has been removed. `dd-catalog` now
defines bounded relationships for equipment, weapons, mesh components, projectiles,
emitters, particle components, and asset leaves. Every weapon follows this shared
profile; no item name or map-specific dependency object name appears in production
validation rules. Actual paths and concrete classes come from source exports.

The IPC worker reads inherited object/component references from source-verified
packages. It starts with the selected cooked source, then permits an external
dependency's named installed package and reviewed English/script packages. Missing
definitions remain errors. It compares the resolved chains across every source
copy of the selected equipment and rejects disagreement. Work is bounded to 64
graph nodes/inheritance steps, 512 source records, and 16 package readers. Asset
leaves use export-table identities without interpreting opaque asset payloads.
Prepared graphs are cached by catalog generation, source path/hash, and item path;
the worker rechecks contributing package provenance on reuse.

Only the game thread inspects native objects. Each request receives its own prepared
graph; no graph is transmitted over the pipe or trusted from the UI. Loaded objects
must match source paths and exact classes, with live object-table membership,
verified reflected property types/targets, and component ownership. Crossbow and
MagicStaff descendants require a projectile through runtime class ancestry.
Optional null references must also be null at runtime. Existing package loading,
map-change reference refresh, session ownership, saveability, and spawn-outcome
checks remain unchanged. This is a bounded weapon-reference validator, not a
recursive verifier of every game asset or an authorization of arbitrary scripts.

Scanner v3 rebuilds cached facts because component properties are now decoded.
The fresh 178-package scan still contains 1,257 entries and 156 catalog-level
exclusions, with zero package issues. Dependency failures additionally block a
selected item during inspection, even when its catalog classification is known.

The initial installed-weapon audit resolved 566 candidates and conservatively
blocked two Assault Pack variants with an empty particle effect. Investigation
confirmed an explicit serialized null overriding the parent's particle asset,
matching a null live pointer and empty alternate-template array. The shipped
`DunDefEmitterSpawnable.PostBeginPlay` explicitly handles this with a failsafe
lifespan (1,340 byte function, SHA-256
`b0d9977e01d2bd6b82128bdce6f3c20be796ba44a7d4c303236997170ff01ada`).
The shared rule therefore permits this optional null leaf, while a nonnull source
reference still requires the exact live asset. Missing inheritance or a missing
nonnull export is never treated as null. This avoids an unnecessary item-specific
exception or blocking a supported empty effect.

The follow-up audit passed all six Assault Pack weapon variants, including both
formerly blocked cases. Together with the full initial audit this resolves all
568 catalog-admitted weapon candidates through the bounded source profile. This
is dependency coverage, not 568 live spawn tests. The final packaged adapter also
passed live inspection of both empty-effect weapons after the game restart.

Live validation passed for both Fenghuang variants and basic crossbow, staff,
spear, and sword templates in the local tavern. The generic validator spawned a
new pickup of each Fenghuang variant; independent readback confirmed the actors,
fresh IDs, and correct original package archetypes. Normal collection increased
the item box from 134 to 136 with all sampled values preserved. After normal exit
and a full restart, both were still inventory-owned with identical IDs, archetypes,
stats, resistances, and upgrades. The game restored their dependency chains before
any adapter was attached in the new process.
The final packaged adapter then revalidated both saved variants and the four basic
weapon families in that process.

| Variant | New equipment IDs | Generic-validator lifecycle result |
| --- | --- | --- |
| `EmeraldCity.01Phoenix` | `1505065856 / 399719232` | Spawn, collection, and full restart verified |
| `EmeraldCity_Small.01Phoenix` | `137573248 / 553344704` | Spawn, collection, and full restart verified |

Synthetic tests cover arbitrary package names, optional nulls, inherited references
and overrides, missing/malformed references, unknown mesh classes, source-class
and reference conflicts, cycles, and component prefixes/bounds. Local evidence
is retained under `target/generic-dependencies` and
`target/item-implementation/lifecycle-generic-*` and is excluded from Git.

Formatting, workspace and i686 adapter Clippy with warnings denied, and all
**114 workspace tests** pass. The installed-package dependency audit is an explicit
opt-in test, separate from synthetic CI fixtures. The packaged real 32-bit DLL
smoke and transport suites also pass. Combat behavior and every random/custom
combination remain outside this verification.

## Package-root identity correction (scanner v2, rules v3)

The item-specific dependency checks described in this earlier verification were
subsequently replaced by the shared validator above.

The old reader omitted the source package from ordinary root exports. This made
distinct map-owned objects appear to share one runtime path, and also affected
nested references. The reader now uses the export table's `ForcedExport` bit:
ordinary roots belong to the source package; forced roots retain their cooked
logical identity; imports retain the external package identity. Classification
uses those resolved paths directly. Scanner v2 invalidates previous package facts;
rules v3 and a new catalog generation keep the overlay and adapter in agreement.

The installed-file rescan reads all 178 packages without issues and discovers
**1,257 distinct entries**: 576 weapons, 66 armor, 327 pets, 221 accessories,
47 collectibles, and 20 world pickups. **156 remain unavailable**. The increase
reflects previously conflated map-local variants, chiefly parent-forcing pets;
it does not establish universal spawn support. The old and new catalogs agree
on availability for every unchanged path. The game-visible cold rebuild took
22.96 seconds, and persisted-cache reload matched the generated snapshot.

Fenghuang now has two entries, `EmeraldCity.01Phoenix` and
`EmeraldCity_Small.01Phoenix`. Their distinct weapon dependencies remain distinct;
no dependency differences are discarded. The adapter checks each variant's live
weapon, projectile, collision component, skeletal mesh, both emitter templates,
and particle assets before creating equipment. It verifies reflected field types
and targets, exact variant object/class identities, component ownership, and live
object references. A failed dependency check prevents creation and is reported
as an unavailable reason by selected-item inspection. The ordinary saveability,
archetype, session, fresh-ID, and dropped-actor checks also remain in force.

Both variants passed these checks in a local single-player tavern on build
25887270. Each produced one independently observed floor actor, with its original
package archetype and a distinct fresh ID pair. Normal collection increased the
item box from 132 to 134; both exact IDs and all sampled stats, resistances, and
upgrade values matched the floor snapshot. Unqualified `01Phoenix` and fabricated
package paths were rejected. After a normal exit and full process restart, both
items remained inventory-owned with the same IDs, class, original archetype,
stats, resistances, and upgrade values. This readback happened before attaching
any adapter in the new process. The game independently restored both map packages
and the weapon/projectile/component/effect chains from the saved item references.

| Variant | Fresh equipment IDs | Spawn, collection, full restart |
| --- | --- | --- |
| `EmeraldCity.01Phoenix` | `1872436480 / 1872741760` | Verified; all sampled values retained |
| `EmeraldCity_Small.01Phoenix` | `997314432 / 1484008832` | Verified; all sampled values retained |

This verifies template-default spawning, storage, and reload in the local tavern.
It does not establish combat behavior or test every random/custom combination.

The authored templates contain out-of-range small weapon bonuses (shots per second
8000 and charge speed 500). Normal game serialization produces 64 and -12 on the
created items. Template-default mode retains this engine behavior; it does not
promise the raw authored numbers survive serialization. Custom controls continue
to enforce the reviewed serialization bounds.

Validation passed: formatting, workspace and i686 adapter Clippy with all targets
and warnings denied, **108 workspace tests**, packaged 32-bit DLL smoke, and
fragmented/oversized/timeout/version-mismatch transport checks. Regression coverage
includes source-owned roots and children, forced cooked roots, external imports,
separate package-local dependency identities, and genuinely conflicting same-path
definitions. Local live evidence is under `target/fenghuang` and
`target/item-implementation/lifecycle-fenghuang-*`; game-derived data stays ignored.

## Initial installed-file comparison and timing (scanner v1)

The Rust scanner read all **178 packages**, totaling **9,700,476,197 bytes**, with
zero package issues. It retains all previous **1,184 equipment candidates** with
matching categories and groups: 575 weapons, 66 armor, 275 pets, 221 accessories,
and 47 collectibles. All 17 previous world records remain; three additional mana
archetypes bring the total to **1,204 entries**, including **110 unavailable**.
Newly discovered templates are not automatically live-tested items.

Measured cold scan: **18.91 seconds**. Warm scan: **1.02 seconds**, reusing all
178 packages. Both produced the same generation and passed persisted-cache
reload comparison. These are local scanner timings, not universal startup bounds.

Conflicting same-path definitions and parent-template restrictions are now
explicit metadata exclusions. Flags and rune/consumable lifecycles remain blocked.
Pirate Mana Token defaults remain available, but its custom stored-value option
is removed because its properties do not establish the reviewed high-digit
initialization. Radioactive coal's high-digit flag alone is insufficient.

## Earlier template-ID conflict fix (rules v2)

This section records the earlier investigation. The scanner-v2 correction and
Fenghuang validation above supersede its unresolved package-root conclusion.

The five reported conflicts were investigated in the installed build's package
metadata, shipped scripts, and current-process reflection/native save routine.
The change excludes only valid signed 32-bit `equipmentTemplateUniqueID` values
from equipment equivalence comparisons. It also applies to inherited definitions.
The field remains in the cached source facts and is never rewritten in the game.
Same-named fields on world pickups and invalidly typed values are not exempted.

Class, archetype identity/flags, stats, save restrictions, and dependency references
remain significant. Conflict messages now name the differing field. Rules version
2 causes automatic reclassification of existing package facts into a new catalog
generation; it does not require reparsing unchanged packages. Both binaries must
be updated together; restart a game that still has the earlier adapter attached.

| Item | Investigation | Result |
| --- | --- | --- |
| Argo | 14 copies; only template ID differs; all force their parent | False conflict removed; parent restriction remains, naming `DunDefEquipment_Familiars.Equipment_Familiar_Dragon` |
| Nagi | 14 copies; only template ID differs; all force their parent | False conflict removed; same Dragon parent restriction |
| Archimedes | 14 copies; only template ID differs; all force their parent | False conflict removed; names `DunDefEquipment_Familiars.Equipment_Familiar_Griffon` |
| Mephisto | 14 copies; only template ID differs; all force their parent | False conflict removed; names `DunDefEquipment_Familiars.Equipment_Familiar_Imp` |
| Fenghuang | Two copies; template ID and package-local `EquipmentWeaponTemplate` references differ | Remains unavailable; conflict now names `EquipmentWeaponTemplate` |

The four pets agree across resolved properties, including nested structures and
arrays (305 tags for Argo/Nagi/Archimedes and 283 for Mephisto). In the shipped
`GetNetInfo`, `bForceUseParentTemplate` makes template selection use the object's
archetype even when self-template selection was requested. The existing adapter
rejects that behavior to preserve the exact selected class/archetype. This fix
does not claim that the four named DLC variants have become spawnable.

Fenghuang's referenced weapon/projectile combat scalars match, but its references
point to distinct map-local objects. Its raw `01Phoenix` export is also unqualified:
static package ownership suggests `EmeraldCity.01Phoenix` and
`EmeraldCity_Small.01Phoenix`, which have not been confirmed as runtime templates.
Dependency equivalence and package-root path normalization are separate unresolved
issues; neither is authorized by ignoring an ID. No map dependencies were loaded
or spawned during this investigation.

Current-build reflection identifies `equipmentTemplateUniqueID` as an `IntProperty`
at offset `0x1e0`; shipped metadata declares it `ObjectID duplicatetransient const`.
The save structure has no corresponding field. The native save routine reads the
object's archetype and writes its path into the saved template field; script loading
resolves that path and constructs the template's actual class with that archetype.
The adapter separately verifies fresh instance IDs (`EquipmentID1/2`). This is
evidence for the narrow comparison exception, not a claim that the template ID is
unused throughout the engine.

Reclassification reused all 178 cached packages in 1.45 seconds and retained all
1,204 entries, with the same capabilities and 110 unavailable entries. Only reasons
and rule labels changed: the four misleading conflicts disappeared, Fenghuang's
remaining difference became explicit, and known forced parents are now named.
Persisted-cache reload matched the new generation.

Rules-v2 validation passed: `cargo fmt --all --check`, workspace and i686 adapter
Clippy with all targets and warnings denied, and **97 workspace tests** (23
adapter, 34 catalog, 21 overlay, 19 protocol). Six new regression tests cover
valid-ID equivalence and preservation of original facts, incompatible properties,
inherited definitions and parent forcing, malformed IDs, distinct archetype
dependencies, and unrelated world-pickup fields. The final portable package also
passed the real 32-bit DLL smoke and transport suites. No new live grant is
claimed for this metadata-only change. Local evidence is retained under
`target/catalog-conflicts` (full resolved comparison and old/new catalog comparison)
and `target/catalog-migration/template-id-*` (script, reflection and native-save
contracts). These ignored files include game-derived metadata/code and stay out of Git.

## Initial rules-v1 verification

Synthetic tests cover parser bounds, unknown classes/types, inheritance, conflicts,
options, cache corruption/version changes, added/removed packages, cancellation,
and mismatched generations. Production code has no fixed catalog-size assertions.
Installed-package evidence is compared separately with the prior research scan.

Automated validation passed: `cargo fmt --all --check`, workspace Clippy with
all targets and warnings denied, the same Clippy check for the i686 adapter,
and all **91 workspace tests** (23 adapter, 28 catalog, 21 overlay, 19 protocol).
The release package passed its real 32-bit DLL smoke test and named-pipe
transport suite, including fragmented/oversized frames, incomplete-request
timeouts, protocol mismatch, and diagnostic catalog-binding rejection.

Read-only preflight of the restarted local tavern compared **1,087 loaded,
statically available templates** with the current Rust classification and
capability rules, with zero differences: 562 weapons, 30 armor, 216 pets,
220 accessories, 47 collectibles, and 12 world pickups. Seven available entries
were not loaded; 110 excluded entries were skipped. This validates metadata
agreement, not the spawn or collection behavior of every template.

The 32-bit streamed parser returned identical metadata to the memory reader for
the installed basic Bracer. Standalone inspection took **0.044 seconds**, with a
sampled peak working set of **45.9 MiB**. This is a parser-process measurement,
not a claim about the game's total memory use.

Live protocol-v6 testing then passed on the restarted, responsive local tavern
(PID 32868), using the packaged DLL unchanged. The game bound the generated
catalog and inspected representatives of all six categories. Golden tokens and
presents reported their missing mission contexts; flags reported their blocked
initialization. Unknown paths and stale generation keys were rejected.

Seven requested pickups were confirmed, each without an ambiguous outcome:

| Representative | Mode/value | Observed result |
| --- | --- | --- |
| Crossbow | Template defaults | Pickup verified; inventory ID matched after collection |
| Chain boots | Current-mission random roll | Pickup verified; inventory ID and rolled values retained |
| Genie | Hero health +23; mana ability field 123 | Actual familiar subclass retained; both values confirmed in inventory |
| Basic bracer | Hero health +17 | Value confirmed in inventory |
| Coal | Template defaults | New inventory identity observed; one unit retained |
| Inventory mana token | Value 123,456 | Matching initialized denomination retained in inventory |
| Beach fruit variant 1 | Packaged defaults | Actor verified; absent after reported collection |

The six inventory items increased item-box records from 120 to 126; their fresh
IDs matched the observed floor objects, and checked stats/class/archetypes/value
fields were unchanged. Fruit buff effects were not sampled during their active
window. This run did not repeat equipping, save/reload, redemption, or mission
map transitions; earlier evidence remains explicitly separate below.

Live negative Spawn requests also rejected an unknown path, a stale generation,
equipment options on coal, a Pirate Mana Token value override, and a flag.
No uncertain dispatched action was retried. These checks establish representative
behavior, not universal spawnability of every discovered entry.

The first attachment attempt reached an already unresponsive game without loading
the DLL. Restarting resolved it. Test cache generation initially inherited Codex's
Windows app-data redirection, so the game could not see that cache. Launching the
same scanner through the existing Explorer desktop populated the normal user
cache and resolved binding without changing the adapter. Normal user launches
should use the same Windows account for the scanner/overlay and game.

Local evidence includes `live-inspection-v6-restarted.json`, `live-spawn-v6.json`,
`live-rejections-v6.json`, and `live-collection-v6.json` in
`target/catalog-migration`; the lifecycle snapshots are in
`target/item-implementation`.
Earlier family behavior/save-reload evidence remains in the
[protocol-v5 report](item-menu-implementation.md), not claimed as a fresh v6 test.
Ignored evidence is in `target/catalog-migration` and the local application cache.
Game assets, saves, and decompiled scripts remain outside Git.
