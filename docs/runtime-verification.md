# Runtime verification

Verified on 2026-10-11 with Steam build 25809881, Win32 executable SHA-256
`4e4b03765c9e74415e27e36d952ebecd86a655ca27d8a46b5285279773e6d0f4`.

REA supplied the earlier artifact identity evidence. The native findings came
from supplemental PE/Capstone analysis of the supported executable, corroborated
with read-only process memory and live adapter execution. No game binaries or
decompiled scripts are included.

## Build-specific bindings

All addresses below are RVAs relative to the loaded executable, not absolute
addresses borrowed from another game or version.

| Binding | RVA | Evidence |
|---|---:|---|
| FName table | 0xD490E4 | Native string formatting/reallocation; live `None` entry and name indices |
| UObject table | 0xD5B41C | Native object iteration; every sampled table slot matched its object index |
| Game thread ID / initialized | 0xCE921C / 0xCE9220 | Native thread initialization/predicate; live window and callback thread equality |
| GEngine | 0xD68368 | Frame-loop update; live DunDefEngine and viewport/player ownership chain |
| UObject ProcessEvent | 0x85E90 | Native caller/callee disassembly; manager vtable slot 65 points here |

The x86 dispatcher uses `thiscall`: ECX is the object, with function, parameters,
and result/null on the stack; it returns with `ret 12`. The adapter calls only
the verified UObject dispatcher on the manager or equipment archetype, requiring
the same vtable target.

Core offsets were confirmed with native instructions and live reflection:
object index 0x04, flags 0x08, outer 0x28, name 0x2C, class 0x34, archetype 0x38;
class superclass 0x3C; property dimension 0x44, size 0x48, flags 0x4C, offset 0x64,
boolean mask 0x84; function flags 0x90, native index 0x94, parameter size 0xA2,
return offset 0xA4. Native `IsTemplate` checks flag 0x400. Gameplay property
offsets come from reflection rather than hard-coded instance layouts.

## Actual operation

The adapter calls the script function
`UDKGame.DunDefHeroManager.ServerCreateDroppedEquipment` with a selected packaged
archetype and the local pawn. Template mode passes quality zero; random mode
passes the selected positive strength. Both set `useEquipmentObjectAsTemplate=true`.
Its reflected parameter buffer is 68 bytes, with the returned actor at byte 64.
All 13 parameter names, types, dimensions, sizes and offsets are checked before
dispatch. Optional values are explicit, including `remoteUserID=-2`.

Only unused archetypes with IDs `(0,0)` are accepted. The duplicate-drop cleanup
option is disabled for these placeholder IDs so creating an item cannot delete
existing drops based on `(0,0)`. The game constructs a new equipment object and
generates its IDs. Success requires the returned actor, world, equipment object,
archetype, and fresh nonzero ID pair to match expectations.

The source catalog contains 858 packaged HeroEquipment references from
Startup_INT.upk (SHA-256
`00656c92c2ad74769210ad914137d93444773057f6d7a3130171e85d42b41568`). The UI lists
592 weapon/armor references whose types were read in the local tavern: 562 weapon
and 30 armor variants. These counts are reference variants, not unique marketed
items. The live checks below cover representative weapons and armor, not the
whole catalog.

## Verified behavior

- Attached the real x86 Rust DLL to the game's window message thread, matching
  the engine's initialized game-thread ID. Engine operations ran on that thread.
- Read back one local player, standalone NetMode, unranked mode, initialized
  manager, and consistent local controller, hero and pawn ownership.
- Spawned one Sword of Embers and one pristine helmet pickup in the regular
  tavern. A separate memory reader confirmed both dropped actors, corresponding
  equipment archetypes, and distinct fresh IDs.
- The user confirmed both pickups appeared and normal pickup worked.
- Disconnected and reattached without stopping the game. An isolated x86 fixture
  separately verified shutdown, stale-request cancellation and host-exit cleanup.

The first live connection refused an array with one player and capacity 17. The
validation now bounds capacity at 128 while requiring exactly one active player;
it does not equate allocation capacity with active player count.

## Custom item creation (v0.2)

The adapter builds the 412-byte EquipmentNetInfo in caller-owned memory using
reflected template properties and the game's signed-value encoding. It calls
`HeroEquipment.CreateEquipmentFromNetInfo`: function flags 0x426002, native index
0, parameters 428 bytes, return pointer at 424. The input structure at 0 is const
out (property flags 0x400182); ownerHero 412, AltOwnerObject 416, fromDroppedEquipment 420.
All structure/property types, dimensions, offsets, sizes, object targets and
boolean masks are checked against the inspected build on attachment.

FString headers borrow unchanged template-owned storage during this synchronous
const-out call. Native ProcessEvent analysis established that its out-parameter
handling does not destroy this caller buffer. InitFromNetInfo makes the owning
copies in the new object. Rust neither owns nor frees Unreal string allocations.
The temporary item is created and passed to spawning in one game-thread callback,
with no frame retention or asynchronous work in between.

The temporary is spawned with quality 0 and useEquipmentObjectAsTemplate=false.
This preserves the original packaged archetype for serialization. Quality and
stats are final before the dropped actor initializes colors, floor categories and
counters. No shared template or existing inventory item is modified. Creation
assigns a fresh ID pair; the final pickup preserves that pair. Success requires
readback of every requested custom value on both the temporary and final item.

Quality indices 0–12 are in reverse rarity order (Godly through Cursed); 13–19 are
Mythical, Transcendent, Supreme, Ultimate, Ultimate93, Ultimate+, Ultimate++.
Hero stat indices 1–10 are exposed; unused index 0 is preserved. Armor resistance
slots are matched by damage class, not assumed positional order. Signed-byte
weapon bonuses encode as value+127 and roundtrip only in −127–128. Hero bonuses
use int32 storage with the same offset, not byte storage. Minimum damage and
maximum-upgrade requirements are template-specific and checked before creation.

Live tests on 2026-10-11 created four pickups:

- Custom Sword of Embers: Mythical, upgrades 1/300, damage bonus 12345, elemental
  damage 2345, knockback 12, hero health 321, hero damage 234, defense damage 456.
- Custom pristine helmet: Ultimate+, upgrades 1/444, ten explicit hero/defense
  bonuses; physical/fire/lightning/poison resistances 42/33/24/15.
- Random pristine helmet at strength 2.0: produced Legendary quality, 20 maximum
  upgrades, randomized stats and resistances.
- Template-default pristine helmet: Torn quality, 6 maximum upgrades and unchanged
  template stats.

An independent process-memory reader confirmed exact custom values, original
archetypes, fresh IDs, and byte-for-byte unchanged template native properties.
A later independent scan found both custom items in the manager's ItemBoxEquipments
array with new instance identities and the same IDs, quality, stats and upgrades.
This verifies pickup/serialization into the item box. It does not establish
save/reload persistence. The pickups had already been collected, so a separate
live readback of floor category membership was unavailable.

## Limits

Only this executable build is supported. Unknown builds, online/ranked sessions,
multiple local players, loading/hero-swap states, invalid templates and changed
ABIs are rejected. Each mutation rechecks session ownership and mode.

Loading absent packages and direct inventory insertion are not implemented.
Save/reload persistence has not yet been verified. The game handles ordinary
pickup and saves. No automatic retry follows
an ambiguous operation result. Not every catalog entry has been spawned.

The adapter DLL is pinned until game exit to prevent a callback or worker from
executing unmapped code. Disconnect removes the host's thread hook and stops IPC;
it does not unload the DLL. Restart the game before replacing an attached build.
# Quality-filtered natural rolls (2026-10-11)

The randomizer's live reflected ABI was checked against the retained script:
`HeroEquipment.AddRandomizeValues`, six 4-byte parameters, flags `0x824002`,
no native index or return value. Each filtered request creates one temporary
equipment object, invokes the normal mission-aware randomizer, and spawns only
an exact quality match. No engine object pointer is retained between requests.

Live local-tavern checks with protocol 3:
- Legendary pristine helmet matched on attempt 7. Independent memory readback
  found one floor pickup and identical candidate/final stats, quality, and
  17 maximum upgrades. Source template bytes stayed unchanged.
- Twenty Ultimate++ misses created no floor pickups.
- The actual overlay search controller cancelled after 15 rolls in 1.04 seconds.
- The actual controller stopped an unsuccessful search at 200 rolls in 13.08 seconds.

Requests are separated by a 50 ms app-side wait. Searches also have a 15-second
deadline, and stop if candidate creation/randomization exceeds 8 ms. The engine
script itself cannot be preempted; these are bounded-work safeguards, not a
hard real-time guarantee. Search unit tests cover pacing, quantity accounting,
attempt limits, cancellation, and stopping on uncertain communication failures.
Save/reload persistence and rare-tier matches remain unverified.

A follow-up game-window message latency probe during 60 nonmatching rolls had
296 samples, zero 250 ms timeouts, and about 16 ms maximum latency (the same
maximum as the baseline). This checks main-thread responsiveness, not rendered FPS.

## Tavern testing profile (2026-10-11)

Protocol 4 adds Current mission / Tavern testing to Random roll. Tavern testing
passes the randomizer's mission override flag and a fixed multiplier of 33.0.
Before rolling, it fills each temporary item's independently allocated difficulty
range array with the last template range, and makes normal/Ruthless promotion
probabilities equal to the template's normal values. Reflected types, offsets,
array element layout, bounds, and non-aliasing are checked before candidate writes.
No game globals, executable code, or source archetypes are changed.

Live single-player tavern tests produced one Mythical pristine helmet on attempt
2 and one Ultimate helmet on attempt 23, with natural stats and one floor pickup
per successful search. Full source object bytes remained unchanged. A separate
Sword of Embers roll with Any quality verified the alternate upgrade calculation:
its candidate used [50, 255] at all five difficulty indices, while the template
and final pickup retained the original per-difficulty configuration. Live game
difficulty remained index 3 and Ruthless remained false before/after that test.

Static inspection of AddRandomizeValues and its randomizer/rating/promotion
helpers identified the mission multiplier, difficulty upgrade range and Ruthless
probabilities as the stat-generation dependencies neutralized by this profile.
The normal generation timestamp still records the actual map. The multiplayer
branch is fixed by the adapter's existing local-single-player guard. This is a
fixed testing distribution using game algorithms, not one specific map's drops.
Rare-tier matching and save/reload persistence have not been exhaustively tested.
