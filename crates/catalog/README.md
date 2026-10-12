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
