# Automatic catalog implementation and verification

Updated 2026-10-12. Protocol **v6** replaces the bundled item catalog with automatic
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

## Installed-file comparison and timing

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

## Template-ID conflict fix (rules v2)

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
