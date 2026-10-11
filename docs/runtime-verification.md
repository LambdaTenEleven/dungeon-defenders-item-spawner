# Runtime verification

Verified on 2026-10-11 with Steam build 25809881, Win32 executable SHA-256
`4e4b03765c9e74415e27e36d952ebecd86a655ca27d8a46b5285279773e6d0f4`.

REA supplied the earlier artifact/provider evidence. Native providers were not
configured. The native findings here came from supplemental PE/Capstone analysis
of the user's installed executable, corroborated with read-only process memory
and live adapter execution. No game binaries or decompiled scripts are included.

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
the verified manager dispatcher and requires the same vtable target.

Core offsets were confirmed with native instructions and live reflection:
object index 0x04, flags 0x08, outer 0x28, name 0x2C, class 0x34, archetype 0x38;
class superclass 0x3C; property dimension 0x44, size 0x48, flags 0x4C, offset 0x64,
boolean mask 0x84; function flags 0x90, native index 0x94, parameter size 0xA2,
return offset 0xA4. Native `IsTemplate` checks flag 0x400. Gameplay property
offsets come from reflection rather than hard-coded instance layouts.

## Actual operation

The adapter calls the script function
`UDKGame.DunDefHeroManager.ServerCreateDroppedEquipment` with a selected packaged
archetype, the local pawn, quality zero, and `useEquipmentObjectAsTemplate=true`.
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
items. Only the two examples below were actually spawned in this verification.

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

## Limits

Only this executable build is supported. Unknown builds, online/ranked sessions,
multiple local players, loading/hero-swap states, invalid templates and changed
ABIs are rejected. Each mutation rechecks session ownership and mode.

Spawns use archetype defaults. Stat/quality editing, loading absent packages,
direct inventory insertion, and save/reload persistence are not implemented or
verified. The game handles ordinary pickup and saves. No automatic retry follows
an ambiguous operation result. Not every catalog entry has been spawned.

The adapter DLL is pinned until game exit to prevent a callback or worker from
executing unmapped code. Disconnect removes the host's thread hook and stops IPC;
it does not unload the DLL. Restart the game before replacing an attached build.
