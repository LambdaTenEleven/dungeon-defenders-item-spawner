# Automatic game-file catalog

`dd-catalog` reads installed gameplay packages without executing game code or
writing game files. It supports verified UE3 formats 678/2 and 679/2, including
bounded LZO1X decompression in safe Rust, with no new dependency.

The scanner first reads package/class tables, then resolves equipment and known
pickup candidates against the complete class graph. Classification resolves
class defaults and archetype inheritance before applying preset rules. A clean
scan requires no previous cache, research files, Python, or bundled item list.

Categories are weapons, armor, pets, accessories, collectibles, world pickups,
and Unclassified. Unknown concrete classes remain unavailable. Type 5 alone
does not identify a pet. Exact reviewed classes define maximum capabilities;
template properties further restrict them. Conflicting definitions, missing
inheritance, unsafe defaults, and consumables remain unavailable. General map
actors are outside candidate discovery.

Equivalent cooked/localized copies are deduplicated; distinct paths retain
variants. Conflicting gameplay definitions sharing one runtime path remain
unavailable. English names are preferred; these can differ from generated names
shown on equipped items.

Scanner v2 uses the export table's `ForcedExport` flag to distinguish cooked
logical roots from ordinary exports owned by the source package. Imports keep
their external identity; nested exports and object references inherit the resolved
root. Thus `EmeraldCity.01Phoenix` and `EmeraldCity_Small.01Phoenix` are separate
variants, without relaxing dependency comparisons. Rules v3 consume these resolved
paths directly. Existing scanner-v1 facts are automatically rebuilt.

Scanner v3 also reads component references and the reviewed component serialization
prefixes. Weapon dependency validation uses a bounded class/relationship profile:
the adapter worker reads expected identities and inherited references from verified
packages, compares the chains across the item's cooked copies, then checks live
objects on the game thread. Item names are not validation rules. Missing required
references, unsupported component classes, malformed/cyclic inheritance, conflicting
chains, and runtime identity/type/owner mismatches block inspection and spawning.

Rules v2 ignore valid signed 32-bit `equipmentTemplateUniqueID` values only when
comparing equipment copies, including inherited defaults. Original cached values
and runtime archetypes are preserved. Class, archetype references, gameplay
properties, save restrictions, and object dependencies must still agree. Conflict
messages identify the differing field. Parent-forcing templates remain unavailable
and name their parent when known; removing an ID-only conflict does not override
that lifecycle restriction.

The cache lives under `%LOCALAPPDATA%/DungeonWorkshop/cache`, separated by
canonical installation identity. Versioned package facts are content-addressed.
Immutable catalog snapshots include source hashes, rules, errors, and generation
identity. Atomic publication preserves the previous cache on cancellation/failure.
Warm scans reuse unchanged package facts; changed inheritance reevaluates all
candidate selection. Rule updates reclassify; scanner/schema updates rebuild.

Size/time are fast change hints. Content hashes bind parsed sources and catalog
generations. The adapter independently verifies source identity and actual game
properties: a cache digest detects corruption but does not grant permission.

Developer verification with a caller-selected cache directory:

```powershell
cargo run --release -p dd-catalog --example scan -- '<installation>' '<cache-directory>' --force
```

Omit `--force` to test incremental reuse. The UI uses the same scanner. Cache and
repository contain metadata, not package assets, saves, or decoded game scripts.
Parser/rule/cache tests use synthetic fixtures.
