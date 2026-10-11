# Native integration status

Investigation date: 2026-10-11. Static analysis only; no game process was modified.

## Analyzed build

- Steam app: 65800; observed build: 25809881.
- Executable: `Binaries/Win32/DunDefGame.exe`, x86 PE.
- Executable SHA-256:
  `4e4b03765c9e74415e27e36d952ebecd86a655ca27d8a46b5285279773e6d0f4`.
- `UDKGame.upk` SHA-256:
  `5f532d3ecf74d85a74be3c6c36ca986a88da1573909ec8525474743479e1e824`.
- Package version 679, licensee version 2, LZO compression.

REA supplied identity/provider evidence. Ghidra and IDA were not configured and
Hopper was unavailable on Windows. Supplemental local format parsing and UELib
UnrealScript decompilation recovered the script path below; this is not native
REA decompilation or runtime proof. UELib commit:
`3207a17e9b294be3d1bf26b18e07ccff7e1d4b0c`.

The analysis used decompressed copies and a native-token name table derived from
shipped packages. The original game files were not rewritten. UELib emitted
property/trailing-data warnings and unrelated deserialization exceptions; native
implementations and runtime layouts remain unverified. Its generated comments
containing `Export UClass::exec...` are not PE export-table evidence.

## Observed item path

The existing reward script obtains an equipment archetype, calls `GetNetInfo`,
creates a temporary equipment object, optionally randomizes it, and invokes
`DunDefPlayerController.GiveEquipmentToItemBox` with its resulting net info.

That client function calls:

```text
DunDefHeroManager.AddEquipmentToItemBox(hero, info, auto_lock)
  -> AddEquipmentFromNetInfo(info, none)
     -> HeroEquipment.CreateEquipmentFromNetInfo(info, ...)
        -> Unreal object allocation from EquipmentTemplate
        -> InitFromNetInfo(info, ...)
  -> assign UserID and FolderID
  -> stack compatible items or append to ItemBoxEquipments
  -> RefreshItemBoxEquipmentLists(hero.UserID)
```

The controller wrapper additionally displays the notification and contains
achievement checks. Calling the manager directly returns the resulting equipment
object and avoids those wrapper-specific effects; it is a candidate for the first
controlled local test, not yet a proven native call.

`EquipmentNetInfo` includes engine object references, strings, arrays, IDs and
encoded stat values. It must be constructed inside the game's address space with
the actual reflected layout. Do not transmute a Rust request into that structure.
The script functions are not direct Windows DLL exports.

Details that the eventual adapter must preserve:

- Resolve the active local hero and manager only after initialization.
- Resolve/load the selected archetype and respect `GetNetInfo`'s template flags.
- Generate new item IDs; `InitFromNetInfo` generates IDs when both supplied IDs
  are zero, while randomization also has a separate ID-generation path.
- The decoded `AddEquipmentToItemBox` body has no capacity check. Apply the game's
  storage policy, including `CanStoreInItemBox`, before a grant.
- The controller wrapper's `bForceEquipmentIntoItemBox` and
  `bOnlyGiveToUniqueProfile` parameters are not used in its decoded body. They
  must not be treated as safeguards merely because of their names.
- Inventory refresh and save are separate operations. The reward sequence calls
  `SaveForEveryone` afterward. A tool should scope any explicit save to the local
  player instead, and must account for normal autosaves.
- The mode enum contains unranked, ranked and non-Steam states. A mode check alone
  does not prove standalone gameplay; verify world/network and local ownership.

## What remains unknown

No verified native dispatcher address, vtable slot, global object table, live
property offsets, game-thread hook, or loading/attachment mechanism is available
yet. Do not borrow offsets from another Unreal game or treat package export
indices as virtual addresses. Native methods must use the exact x86 calling
convention and verified parameter/return/out layouts. Engine-owned strings and
objects also require engine lifetime management.

The current adapter therefore exposes only diagnostic status and rejects grants.
It exports an explicit `dd_adapter_run` entry point; the isolated host calls it
after `LoadLibraryExW` returns. No work is performed under `DllMain`, and the host
unloads the DLL only after its service returns with no background threads.

An UnrealScript/TcpLink mod adapter is an alternative for custom maps, but does
not meet the chosen regular-map workflow. The production direction remains an
external Rust UI with a small native Rust adapter in the running game.

## References

- [Rust Windows targets](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html)
- [Rust dynamic system libraries](https://doc.rust-lang.org/reference/linkage.html)
- [UELib](https://github.com/EliotVU/Unreal-Library)
- [DDDK](https://store.steampowered.com/app/202522/Dungeon_Defenders_Development_Kit_Free_DLC/)
