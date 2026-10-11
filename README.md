# Dungeon Workshop

A small Rust overlay and native adapter for experimenting with equipment in
Dungeon Defenders single-player.

**Status: working single-player preview for Steam build 25809881.** Choose
equipment and spawn it beside your hero, then pick it up normally. The weapon
and armor spawn path was verified in a regular tavern, including user-confirmed
pickup. The catalog contains 592 weapon/armor references; not every entry has
been individually tested. Items use their template's default stats.

## Run the preview

1. Extract the complete portable ZIP into any writable folder.
2. Start Dungeon Defenders and enter a regular **local single-player** map/tavern.
3. Run `dd-overlay.exe` and click **Connect to game**. The first connection briefly
   pauses gameplay while inspecting reflection metadata.
4. Search/select equipment, choose quantity, and click **Spawn pickup**. Pick it
   up in-game to equip or store it. Normal game saves can retain picked-up items.

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
spawning. Other builds and ranked/online sessions are unsupported. Templates
must already be loaded; reconnect after changing maps if a reference is missing.

Closing the overlay disconnects its helper without closing the game. The adapter
DLL stays loaded and idle until game exit, so restart the game before updating
or deleting an attached release. Uninstall by closing both apps and deleting
the extracted folder. Nothing is installed into the game's directory.

## Build

Windows 10/11 x64, Rust 1.99.0, and Visual Studio Build Tools with the x86/x64 C++
tools and Windows SDK are required for building. The Rust toolchain file pins the
compiler and targets. End users do not need these development tools.

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
powershell -ExecutionPolicy Bypass -File scripts/package.ps1
```

`scripts/package.ps1` builds the x64 overlay and x86 adapter/host, runs the DLL
integration smoke test, and produces a portable ZIP and SHA-256 checksum in
`dist/`. The CRT is linked statically. The script packages an explicit list of
files, never game binaries or assets.

To test an extracted release without opening a window:

```powershell
.\dd-overlay.exe --self-test
.\dd-overlay.exe --check-game 'D:\SteamLibrary\steamapps\common\Dungeon Defenders'
```

The test loads the real DLL, verifies the server process and architecture, checks
invalid input and version rejection, confirms the isolated host cannot spawn, then
requests shutdown and checks that the host exits successfully.
Packaging additionally exercises fragmented messages, oversized frames and
stalled clients against the real DLL using `scripts/transport-test.ps1`.

## Design

- `crates/overlay`: egui/eframe window, equipment search, game identification and
  helper lifecycle. Connections and requests run outside the UI thread.
- `crates/protocol`: concrete versioned messages and Windows named-pipe transport.
  Requests contain semantic item paths and quantities, never game pointers.
- `crates/adapter`: build validation, a thread-specific Windows message hook,
  reflected engine bindings, local-session guards and confirmed pickup creation.

The x86 helper loads the adapter using a thread-specific `WH_GETMESSAGE` hook in
the verified game's window thread. Native analysis and runtime reads established
that it matches the engine's game thread. Requests travel from the pipe worker
through a bounded queue to that callback. Expired queued requests cannot execute
later, and an ambiguous running operation is never retried automatically. There
is one attachment per game process. The DLL is pinned to make disconnect safe.

The pipe rejects remote clients, verifies the server PID on connection, limits
frames to 16 KiB, and applies a two-second I/O deadline. One connection carries
one request and response. The protocol is deliberately small; it has no arbitrary
command execution or memory-write operation.

## Current limits

- Supports one local player on the verified Windows x86 game build.
- Creates floor pickups with template stats; no custom stat/quality editing.
- No direct inventory insertion, package loading, pets or accessories.
- Save/reload persistence and every catalog variant have not been tested.
- After an uncertain response, inspect nearby pickups before trying again.

See [runtime verification](docs/runtime-verification.md) for the checked bindings,
live results and limits, and [the original investigation](docs/integration.md)
for the item-box route. No custom map or Total Conversion is required.

This repository contains original tool code and equipment object-path references;
it does not redistribute Dungeon Defenders assets, scripts or decompiled code.
