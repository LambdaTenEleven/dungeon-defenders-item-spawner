# Dungeon Workshop

[![Rust checks](https://github.com/LambdaTenEleven/dungeon-defenders-item-spawner/actions/workflows/ci.yml/badge.svg)](https://github.com/LambdaTenEleven/dungeon-defenders-item-spawner/actions/workflows/ci.yml)
[![Rust 1.99.0](https://img.shields.io/badge/Rust-1.99.0-black?logo=rust)](rust-toolchain.toml)
![Windows 10/11 x64](https://img.shields.io/badge/Windows-10%2F11%20x64-0078D4)
![Development preview](https://img.shields.io/badge/status-development%20preview-orange)

Spawn items beside your hero in Dungeon Defenders, then collect them normally.
Supports weapons, armor, pets, accessories, collectibles, and world pickups.

**Development preview — Windows 10/11 x64, Steam build 25887270, local
single-player only.** Other game builds and ranked/online sessions are unsupported.

## Get started

1. Extract the complete portable ZIP into a writable folder. Keep
   `dd-overlay.exe`, `dd_adapter.dll`, and `dd-adapter-host.exe` together.
2. Start Dungeon Defenders in windowed or borderless mode and enter a local
   single-player map or tavern.
3. Run `dd-overlay.exe`, wait for the item scan, and click **Connect to game**.
4. Search by name or object path, filter by category, and select an item. Once
   its in-game inspection passes, choose options and a quantity of **1–10**, then
   click **Spawn pickup**.
5. Collect the pickups in-game to equip or store them. A successful spawn confirms
   a floor pickup; collection and saving happen through the game.

No installer, Rust runtime, administrator rights, or game-folder changes are
required. The game path is detected automatically; change it in **Settings** if
needed. Exclusive fullscreen and a global toggle hotkey are not supported.

## Item options

Equipment offers these modes where supported:

- **Template defaults:** use the item's original values.
- **Random roll:** let the game generate stats. Choose **Any** quality for an
  ordinary roll, or a specific tier to search for an exact match. Some templates
  cannot produce every tier.
- **Custom:** start from template defaults and override only checked fields.
  Weapon controls are bonuses, so they may differ from displayed totals. Pet
  abilities and accessory resistances depend on the selected item's capabilities.

For random rolls, **Current mission** uses the map's loot modifiers.
**Tavern testing** uses a fixed high-level testing profile without changing the
map's difficulty. Adjust roll strength beside the roll options. Search limits are
also available in **Settings**. Searches stop at **200 attempts or 15 seconds** by default;
limits can be set to 1–10,000 attempts and 1–300 seconds and are saved automatically.
Both limits cover the entire requested quantity. **Cancel search** stops after
the current request finishes; partial results remain on the floor.

Collectibles offer quantity and, for supported mana tokens, stored value. Collect
and redeem them normally. World pickups offer quantity and packaged variants;
mission pickups may require their original mission controllers.

Invalid values disable spawning. After a communication error, check for nearby
pickups before trying again: an uncertain spawn is never retried automatically.

## Catalog and limits

The catalog is scanned from your installed game files and cached for later
launches. Changed packages are rescanned. Use **Rebuild catalog** for a fresh scan
or **Cancel scan** to keep the previous cache. Cache and search settings live under
`%LOCALAPPDATA%\DungeonWorkshop`.

Unknown item types remain visible but unavailable. Conflicting definitions,
unresolved dependencies, and unsupported behavior also block spawning, with a
reason shown in the item details. Discovery alone does not establish support for
every variant or map.

Flags remain unavailable; golden tokens and presents need their mission
controllers. Runes lack usable verified templates. Costumes and crystal skins are
unlock systems. Representative collection, equipping, and save/reload checks have
passed; collectible redemption remains unverified. See the
[catalog report](docs/automatic-catalog.md) and
[item-family report](docs/item-menu-implementation.md) for coverage and limitations.

Closing the overlay leaves the adapter idle inside the game. **Exit the game
before replacing or deleting an attached release.** To uninstall, close both apps
and delete the extracted folder.

## Build from source

Install [Rust through rustup](https://rust-lang.org/tools/install/) and Visual
Studio Build Tools 2022 with **MSVC v143 C++ x64/x86 build tools** and
**Windows 11 SDK (10.0.22621.0)**. The repository pins the Rust toolchain and targets;
missing components are downloaded on first use.

Double-click `build.cmd`, or run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\build.ps1
```

The script runs formatting checks, Clippy, workspace tests, release builds, and
32-bit adapter smoke/transport tests. It writes a portable ZIP and SHA-256 checksum
to a new timestamped folder under `dist/`. Use `-OutputDirectory <fresh-folder>` to
choose another location. The first build requires internet access.

To check an extracted release:

```powershell
.\dd-overlay.exe --self-test
.\dd-overlay.exe --check-game '<game-installation-folder>'
```

The self-test runs in an isolated host without connecting to the game or spawning
items. The game check inspects the installation.

## AI usage disclosure

This app was made using Codex and Claude just to test the REA skill - https://github.com/morluto/rea
