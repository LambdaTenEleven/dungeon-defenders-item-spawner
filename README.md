# Dungeon Workshop

A small Rust overlay and native adapter for experimenting with items in
Dungeon Defenders single-player.

**Status: development preview targeting Steam build 25887270, protocol v6.**
Choose an item and spawn it beside your hero, then collect it normally. The item
catalog is built automatically from your installed game packages. No item list
is bundled. Categories are Weapons, Armor, Pets, Accessories, Collectibles,
World Pickups, and Unclassified. Unknown candidates remain visible with an
unavailable reason and cannot be spawned.

Duplicate equipment copies may have different cooked template IDs; those IDs do
not cause a conflict by themselves. Other property or dependency differences
remain blocked. Parent-forcing items show the parent template to select instead.

The first scan runs in the background. Later launches reuse cached package
metadata and reparse changed files. **Rebuild catalog** forces a fresh scan;
**Cancel scan** preserves the previous cache. Metadata is stored under
`%LOCALAPPDATA%\DungeonWorkshop\cache`, never in the game installation.

Automatic discovery does not establish new engine behavior. Native integration
still requires the verified executable and script packages, a standalone local
session, and live inspection of each selection. See the
[automatic catalog report](docs/automatic-catalog.md) for scanner coverage,
cache behavior, current validation, and known limits. The
[earlier item-family acceptance report](docs/item-menu-implementation.md)
records the protocol-v5 representative collection and save/reload evidence.

## Run the preview

1. Extract the complete portable ZIP into any writable folder.
2. Start Dungeon Defenders and enter a regular **local single-player** map/tavern.
3. Run `dd-overlay.exe` and wait for the package scan to finish, then click
   **Connect to game**. The first connection validates script identities and
   reflection metadata. Scanning itself does not attach to or modify the game.
4. Search names or object paths, filter by category and family/slot, and select
   an item. Wait for its running-game inspection. Equipment offers **Template
   defaults**, **Random roll**, or **Custom**, where supported. Choose 1–10
   pickups and click **Spawn pickup**.
5. Pick it up in-game to equip or store it. Collected items appear in your item
   box. Normal game saves can retain picked-up items.

Install path, disconnect and the isolated adapter test are under **Settings**.
Status and search progress appear in the bar at the bottom of the window.

In **Random roll**, choose **Any** quality for an ordinary roll, or select a tier
to search naturally generated rolls for an exact match. The strength slider
(0.01–2.0), template settings and selected **Loot conditions** determine the rolls.
The tool never relabels an unsuccessful roll or changes its stats to force a match.
Some templates cannot produce every tier or disable randomization entirely.

**Current mission** preserves the game's current map/difficulty modifiers.
**Tavern testing** uses a fixed high-level testing profile anywhere: a 33.0 mission
multiplier override, each template's last configured difficulty-specific upgrade
range, and its normal (non-Ruthless) Ultimate promotion probabilities. The game
still rolls stats, negative bonuses, upgrade counts and quality itself. This is a
testing profile, not a reproduction of one particular map's loot distribution.
It changes only newly created temporary equipment, never live game difficulty or
shared templates. **Any** quality also works with Tavern testing; multiple items
are paced one at a time. Unreachable tiers can still exhaust the search limit.

Searches stop after 200 candidates or 15 seconds across the requested quantity.
The app waits at least 50 ms between single-candidate game-thread requests;
rejected candidates never become floor pickups. **Cancel search** stops further
attempts after the current request finishes. Progress and the final message report
how many pickups were actually spawned, including partial results. Communication
errors stop the search without retrying an uncertain spawn. A candidate taking
over 8 ms stops the search; an individual game script call cannot be interrupted.

In equipment **Custom**, select a quality or keep **Template**, then check only the fields
you want to override. Unchecked values remain inherited. Hero/defense bonuses,
armor resistances, weapon bonuses and upgrade counts are supported. Weapon
controls are **bonuses**, not final displayed totals; a weapon may not use every
bonus. Elemental damage keeps the template's element. Special items can retain
their fixed name and beam color even with a different quality tier.

Type quantities and custom values directly into their text boxes. Adjust roll
strength with the slider.
Dragging selects text without changing the number. Empty, incomplete and
out-of-range entries disable spawning until corrected.

The editor bounds hero bonuses to ±9999, damage/ammo to 0–1000000, projectile
speed bonus to ±1000000, resistances to 0–100, and upgrades to 1–9999. These are
tool limits, not normal loot caps. Small weapon bonuses are limited to −127–128
because larger values wrap in the game's serialization. The adapter also checks
template minimum damage/upgrades and rejects a current level above the maximum.
Leave both upgrade fields unchecked to preserve template defaults.

Pets expose reviewed family-specific damage, healing, mana, guardian boost/range,
attack rate, and projectile bonuses. Families without reviewed ability mappings
offer common equipment controls only. Accessories expose resistances only when
their templates support them. Changing category or capability profile clears
incompatible settings; the adapter also rejects incompatible requests.

Collectibles expose quantity, plus an optional stored value of 1–2,000,000,000
for inventory mana tokens whose value initialization is verified. Pirate mana
tokens currently expose defaults only. Collectibles have no equipment quality or combat controls.
Collect and redeem them normally. World pickups expose packaged variants and
quantity. Loose mana and fruit buffs have a separate actor-spawn route. Golden
tokens require their mission controller; presents require delivery controllers.
Flags remain unavailable because their team/base initialization is unverified.
Physical spawning does not create mission progress or prove successful collection.

Keep the three matching binaries together;
restart the game before replacing an older attached adapter.

**Run isolated adapter test** is a separate diagnostic; it never connects to the
game or creates equipment. **Check** only inspects the installation.

Keep these three files together:

```text
dd-overlay.exe          # 64-bit desktop UI
dd_adapter.dll          # 32-bit native Rust adapter
dd-adapter-host.exe     # 32-bit attachment helper and isolated test host
```

No installer, Rust runtime, administrator rights, or changes to the game's folder
are required to run this preview. Use windowed or borderless gameplay for the
external always-on-top panel. Exclusive fullscreen rendering and a global toggle
hotkey are not implemented.

The game path is detected from common Steam libraries; it can also be entered
manually. Detection checks PE architecture and the executable's SHA-256. A
recognized executable is required for attachment. The adapter also checks live
reflection layouts, game thread, standalone mode and local hero ownership before
spawning. Other builds and ranked/online sessions are unsupported. Missing
catalog assets can be loaded through the game loader from identity-checked,
scanned source packages with matching content hashes and reviewed classes.
References are refreshed after loading and map
changes. Unresolved dependencies produce an explicit rejection.

Closing the overlay disconnects its helper without closing the game. The adapter
DLL stays loaded and idle until game exit, so restart the game before updating
or deleting an attached release. Uninstall by closing both apps and deleting
the extracted folder. Nothing is installed into the game's directory.

## Build

Windows 10/11 x64 is required. Install these development tools once:

1. Install [Rust using rustup](https://rust-lang.org/tools/install/) with the MSVC
   toolchain. Rust lives in `%USERPROFILE%\.cargo` and `%USERPROFILE%\.rustup`.
2. In Visual Studio Installer, install or modify **Build Tools 2022**, selecting
   **MSVC v143 C++ x64/x86 build tools** and **Windows 11 SDK (10.0.22621.0)**.
   These are installed in the standard Program Files locations.

The repository's `rust-toolchain.toml` selects Rust 1.99.0, rustfmt, Clippy, and
both Windows MSVC targets. Rustup downloads missing components on first use.
End users of the portable ZIP do not need development tools.

Double-click **build.cmd**, or run this command from the repository:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\build.ps1
```

The script checks formatting, runs Clippy for the overlay and 32-bit adapter,
runs workspace tests, and builds and tests the release package. Each build goes
into a new timestamped folder under `dist/`, containing the portable ZIP and its
SHA-256 checksum. Compilation is cached in `target/`. No Codex folder or tools
are required. The first build needs internet access to download Rust components
and Cargo dependencies.

Optionally pass `-OutputDirectory <fresh-folder>` to choose an output location.
For packaging without the initial formatting, Clippy, and unit-test checks, use
`scripts/package.ps1 -OutputDirectory <fresh-folder>`; this still runs the adapter
smoke and transport tests. The CRT is linked statically. Packaging uses an
explicit file list and never includes game binaries or assets.

To test an extracted release without opening a window:

```powershell
.\dd-overlay.exe --self-test
.\dd-overlay.exe --check-game '<game-installation-folder>'
```

The test loads the real DLL, verifies the server process and architecture, checks
invalid input and version rejection, confirms the isolated host cannot spawn, then
requests shutdown and checks that the host exits successfully.
Packaging additionally exercises fragmented messages, oversized frames and
stalled clients against the real DLL using `scripts/transport-test.ps1`.

## Design

- `crates/overlay`: egui/eframe window, equipment search, game identification and
  helper lifecycle. Connections and requests run outside the UI thread.
- `crates/catalog`: bounded package reader, preset class rules, and installation-specific metadata cache.
- `crates/protocol`: concrete versioned messages and Windows named-pipe transport.
  Requests contain semantic item paths, quantities and typed item options, never
  game pointers.
- `crates/adapter`: build validation, a thread-specific Windows message hook,
  reflected engine bindings, local-session guards and confirmed pickup creation.

The x86 helper loads the adapter using a thread-specific `WH_GETMESSAGE` hook in
the verified game's window thread. Native analysis and runtime reads established
that it matches the engine's game thread. Requests travel from the pipe worker
through a bounded queue to that callback. Expired queued requests cannot execute
later, and an ambiguous running operation is never retried automatically. There
is one attachment per game process. The DLL is pinned to make disconnect safe.

The pipe rejects remote clients, verifies the server PID on connection, limits
frames to 16 KiB, and applies a two-second framing deadline. A complete, valid
v6 catalog-binding, inspection, or spawn request receives a 30-second response
budget for catalog/source verification and package loading; incomplete
or invalid requests do not extend the deadline. One connection carries
one request and response. The protocol is deliberately small; it has no arbitrary
command execution or memory-write operation.

## Current limits

- Supports one local player on the verified Windows x86 game build.
- Creates floor pickups with defaults, random rolls, or explicit custom overrides.
- Custom mode starts from template defaults; it does not reroll before applying overrides.
- No direct inventory insertion. Success means a verified floor actor, not an inventory grant.
- Representative collection, pet/accessory equipping, custom values, and save/reload passed; redemption remains unverified.
- Normal pickup stores coal separately; save/reload merged the three tested coal items into one stack.
- Flags and mission pickups outside their required controllers remain blocked.
- Runes lack concrete usable templates; costumes and crystal skins are unlock systems.
- Catalog coverage does not imply that every variant works in every map.
- After an uncertain response, inspect nearby pickups before trying again.

See [current-build evidence](docs/current-build-validation.md),
[catalog scanning](crates/catalog/README.md),
[historical runtime verification](docs/runtime-verification.md), and [the original investigation](docs/integration.md)
for the item-box route. No custom map or Total Conversion is required.

This repository contains original tool code and equipment object-path references;
it does not redistribute Dungeon Defenders assets, scripts or decompiled code.
