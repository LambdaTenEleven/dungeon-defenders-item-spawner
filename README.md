# Dungeon Workshop

A small Rust overlay and native adapter for experimenting with equipment in
Dungeon Defenders single-player.

**Status: development preview. The UI and 32-bit adapter transport work; giving
items in the game is not implemented yet.** The Give button is disabled. The
adapter also rejects grant requests, so bypassing the UI cannot report a fake
success or call unverified game addresses.

## Run the preview

1. Extract the complete portable ZIP into any writable folder.
2. Run `dd-overlay.exe`.
3. Use **Check** to inspect your game installation and **Test adapter connection**
   to exercise the actual 32-bit DLL in a separate diagnostic process.

Keep these three files together:

```text
dd-overlay.exe          # 64-bit desktop UI
dd_adapter.dll          # 32-bit native Rust adapter
dd-adapter-host.exe     # 32-bit isolated adapter test host
```

No installer, Rust runtime, administrator rights, or changes to the game's folder
are required to run this preview. Use windowed or borderless gameplay for the
external always-on-top panel. Exclusive fullscreen rendering and a global toggle
hotkey are not implemented.

The game path is detected from common Steam libraries; it can also be entered
manually. Detection checks PE architecture and the executable's SHA-256. A
recognized executable means it matches the analyzed file, **not** that the item
integration has been verified. The starter item references have not been tested
for runtime availability.

The diagnostic host never opens the game process. Closing the UI ends its own
diagnostic child. Uninstall by closing the app and deleting its extracted folder.

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
invalid input and version rejection, confirms item grants are unavailable, then
requests shutdown and checks that the host exits successfully.
Packaging additionally exercises fragmented messages, oversized frames and
stalled clients against the real DLL using `scripts/transport-test.ps1`.

## Design

- `crates/overlay`: egui/eframe window, read-only game identification, test-host
  lifecycle. Slow checks and connections run outside the UI thread.
- `crates/protocol`: concrete versioned messages and Windows named-pipe transport.
  Requests contain semantic item paths and quantities, never game pointers.
- `crates/adapter`: DLL entry point, request validation and capability reporting.
  There is no engine hook, injection code, or invented object layout.

The pipe rejects remote clients, verifies the server PID on connection, limits
frames to 16 KiB, and applies a two-second I/O deadline. One connection carries
one request and response. The protocol is deliberately small; it has no arbitrary
command execution or memory-write operation.

## Next implementation milestone

1. Verify object lookup, the Unreal function dispatcher, parameter layouts and a
   game-thread dispatch point against the current 32-bit executable.
2. Implement adapter loading and a read-only handshake in regular single-player.
3. Verify local player, standalone world and unranked/local mode before enabling
   a single item-box grant. Unknown state must reject the request.
4. Confirm inventory contents, item IDs and displayed stats after the call.
5. Verify save/reload behavior on a backed-up local profile before adding batches
   or stat editing. Merely skipping an explicit save does not prevent autosaves.

See [the investigation notes](docs/integration.md) for the observed game methods
and the remaining native work. The aim is normal single-player maps without a
custom map or Total Conversion.

This repository contains original tool code and a few object-path references;
it does not redistribute Dungeon Defenders assets, scripts or decompiled code.
