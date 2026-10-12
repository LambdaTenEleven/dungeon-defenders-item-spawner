# Current executable integration evidence

Inspected on 2026-10-12 for Steam build 25887270. This document records static
findings separately from runtime validation. It is not a claim that new item
families have been spawned successfully.

## Identity and method

The installed `Binaries/Win32/DunDefGame.exe` is an x86 PE with preferred image
base `0x400000` and SHA-256:

`2b43fef8ecf629d72b9a3fc032a65d6bf0298fd983301e97cdbfb45fd6314883`

REA `open_binary` independently reported that identity. REA native providers
were unavailable: Ghidra and IDA were not configured, and Hopper does not support
this Windows host. Supplemental read-only PE parsing and Capstone disassembly
were used. No providers were installed. The old build's retained instruction
sequences were used only as search signatures: address-bearing operands were
masked, each signature had one match in the current image, and the matching
current instructions and imported API identities were inspected independently.
The old addresses do not contain the old functions in this image.

The full local evidence is in ignored `target/item-implementation/`:
`current_matches.json`, `current-imports-strings.json`, the corresponding
`*-0.txt` disassemblies, and `current_runtime_probe.py`. Game binaries and
reconstructed game source are not included in this repository.

## Build-specific addresses

All bindings are RVAs relative to the actual loaded executable base.

| Binding | Current RVA | Current-image evidence at preferred VA |
|---|---:|---|
| FName table | `0xD500E4` | `0x4A9315` indexes the data pointer; `0x4B1F73` through `0x4B1FBC` update count, capacity, and allocation |
| UObject table | `0xD6241C` | `0x459A93` compares count and `0x459AAB` indexes the data pointer |
| Game thread ID | `0xCF021C` | `0xC6A77E` calls imported GetCurrentThreadId and `0xC6A79B` stores its result |
| Game thread initialized | `0xCF0220` | `0xC6A7A0` stores one; `0x5D6210` tests initialization before comparing current thread |
| Engine pointer | `0xD6F368` | `0xC676E7` loads the engine before virtual frame update at vtable byte offset `0x128` |
| UObject ProcessEvent | `0x863E0` | Complete dispatcher body at `0x4863E0` through `0x4866D0` |
| Actor ProcessEvent override | `0x210030` | `0x610030` through `0x610076`, forwarding to the UObject dispatcher |
| Window message pump | `0xD91F0` | Imports PeekMessageW, TranslateMessage, DispatchMessageW; frame loop calls it at `0xC677DF` |

The thread predicate uses the current PE's GetCurrentThreadId IAT entry at
`0xE3D1BC`. The frame update and message-pump call share the inspected frame
routine. Runtime must still require the callback thread, engine global thread,
and selected game window thread to agree.

## Dispatcher and layouts

Current native instructions confirm x86 thiscall dispatch: object in ECX;
function, parameter buffer, and optional result pointer on the stack; callee
returns with `ret 12`. A reflected event wrapper at `0x75D740` calls vtable byte
offset `0x104` (slot 65). Actor objects have an override and must not blindly use
the manager's UObject dispatcher identity.

Current instructions confirm the following structural fields used by the
adapter:

- UObject: internal index `0x04`, flags `0x08`, outer `0x28`, FName `0x2C`,
  class `0x34`; class superclass `0x3C`.
- FNameEntry: wide-string flag at `0x08` bit 0, text at `0x10`.
- UProperty: dimension `0x44`, element size `0x48`, flags `0x4C`, offset `0x64`.
- UFunction: flags `0x90`, native index `0x94`, parameter size `0xA2`,
  return offset `0xA4`, native implementation pointer `0xAC`.
- Native IsTemplate implementation at `0x47F490` tests UObject flag `0x400`.

ProcessEvent allocates the reflected property size, copies the reflected
parameter size, supplies the return address from the reflected return offset,
and dispatches the function's native implementation. Its current native-index
check explicitly refuses nonzero indices. Gameplay instance offsets, boolean
masks, reflected object targets, function parameter contracts, and instance
ownership must be checked against live reflection rather than inferred from
these structural fields alone.

## Object loading and world spawning

Current installed Core.upk SHA-256 is
`e68c9e282b5bdef4c526db62588e6d748e397e66e355f0dbd4abf8ef6f9fe7de`.
Current installed Engine.upk SHA-256 is
`7c2fb919b6a5f8d73b3ea472180f30234d70e98311c589f2a5442d42e3ea007d`.
Both match the retained decoded script evidence; these are current file checks,
not an assumption that all packages survived the executable update unchanged.

The shipped `Engine.Actor.Spawn` has native index zero and function flags
`0x24401`. It accepts a Class<Actor>, optional owner, name tag, position vector,
rotation, actor template, and collision-failure flag, and returns Actor. The
current native registration table binds it to execSpawn RVA `0x44B860`.
That implementation reads the declared seven parameters, calls the world spawn
routine, and writes a returned actor pointer. This route can preserve a selected
actor archetype by passing ActorTemplate. No new ad hoc FFrame or raw world-spawn
ABI should be constructed when the reflected route is available.

The shipped `Core.Object.DynamicLoadObject` also has native index zero, with
function flags `0x4026401`. It accepts an object-name string, expected class,
and optional MayFail boolean, and returns Object. The current registration
binds it to execDynamicLoadObject RVA `0x859C0`. Its implementation forwards
those values to the native loader and destroys its own temporary FString.
Use catalog-allowlisted names, MayFail=true, and an explicitly checked expected
class. Verify loaded object identity and refresh object references after loading.
FString ownership and all parameter offsets require live ABI validation before
calling it from the adapter.

These findings establish feasible normal ProcessEvent routes for both methods.
They do not establish that a particular package is loadable, that mission actors
work outside their missions, or that their initialization has no dependencies.

## Runtime status

The game was initially closed. A later read-only probe of PID 12704 observed the
current executable in `LobbyLevel_Normal`, with 77,961 names and 249,971 object
records. Every observed object-table index matched. Game thread 9656 matched
the game-window thread; its initialization flag was one. The manager used
ProcessEvent RVA `0x863E0`; the pawn, controller, and WorldInfo used the Actor
override RVA `0x210030`.

The single local player's controller, hero, pawn, and world ownership checks
passed. NetMode and CurrentMultiplayerMode were zero, the manager was initialized,
the hero was local, and neither hero loading nor swapping was active.

Live function reflection matched Spawn (52-byte parameters, return offset 48),
DynamicLoadObject (24/20), CreateEquipmentFromNetInfo (428/424), and the manager's
equipment spawning method (68/64), including the checked parameter types, flags,
dimensions, offsets, and targets. All nine observed familiar randomizer overrides
had native index zero, flags `0x24002`, 24-byte parameters, and no return value.
Their base implementation had the expected flags `0x824002`.

The supplemental `validate_runtime_layout.py` compared every one of the 118
properties declared in `engine/layout.rs`, the 27 declared engine fields, and
the 540/412-byte native-equipment/net-info structure sizes against live memory.
There were zero mismatches. Evidence is in `current-runtime-validation.json`,
`current-runtime-objects.json`, and `current-runtime-layout.json` under the
ignored analysis directory. This worker performed no engine calls, injections,
target memory writes, spawns, or save changes.

The bounded read-only probe `target/item-implementation/current_runtime_probe.py`
accepts a PID, checks the executable hash, verifies the None name and every
observed object-table slot index, records reflected function/parameter contracts,
and checks local-player ownership and game-window thread identities. It writes
only ignored analysis JSON files. Runtime dispatch, drops, collection, save/reload,
and mission pickup lifecycle remain separate acceptance tests.

## Loader string ownership (current-image static trace)

A Rust-owned, null-terminated UTF-16 vector may back the ObjectName FString
header for a synchronous DynamicLoadObject ProcessEvent call, provided it stays
alive until return and the reflected normal input-parameter contract is checked.
This does not authorize transferring the buffer to an engine-owned FString.

The traced ownership chain is:

1. ProcessEvent `0x4864F0` copies ParmsSize bytes into its temporary frame.
2. Its cleanup at `0x486620` compares each property's offset with ParmsSize.
   Only locals at or beyond ParmsSize receive a destructor call through vtable
   offset `0x170`. Non-out parameters below ParmsSize are copied back without
   destroying the caller's buffer (`0x486650` through `0x48666A`).
3. Bytecode NativeParm (`0x29`) is registered to `0x472640` by current startup
   code `0xDFD410`. Normal input parameters invoke the property's copy method
   through vtable offset `0x164`.
4. The current StrProperty constructor at `0x44AFA0` sets vtable `0xE47E70`.
   That table's copy method points to `0x4C82E0`, which calls its single-value
   method at `0x4D12A0`. The latter invokes FString assignment `0x436920`.
5. FString assignment allocates/resizes the destination's own buffer and copies
   the UTF-16 characters. execDynamicLoadObject destroys this independent local
   copy at `0x485ACE`; it does not own the caller's parameter storage.

Local disassembly files `execNativeParm.txt`, `strproperty-0x164.txt`,
`strproperty-copy-single.txt`, `fstring-assign.txt`, `execDynamicLoadObject.txt`,
and `processevent-0.txt` record the trace. Runtime still needs exact reflected
parameter offsets, dimensions, flags, and native-index validation before calls.

## Loading a cooked source package

`DynamicLoadObject("DD_Christmas", class'Package', true)` is not a working
whole-package loader in this build. Its native wrapper passes a null Outer to
StaticLoadObject (`0x4C76A0`). ResolveName (`0x4BEF70`) returns on a name without
`.` without assigning Outer (`0x4BF430` to `0x4BF741`); StaticLoadObject then
rejects the null Outer at `0x4C7703`. There is no Package-class special case.
A fully qualified archetype path can use DynamicLoadObject when its logical
package can be resolved, but this does not preload a differently named cooked
container automatically.

A separate current native LoadPackage routine is present at **RVA `0xC7C60`**:

`cdecl(Outer: UObject*, Filename: const UTF16*, LoadFlags: u32) -> UPackage*`

Its current-image body takes three stack arguments and returns with plain `ret`.
Fifteen current direct callers corroborate this calling convention. In particular,
`0x457764` pushes flags `0x2002`, the UTF-16 filename, and null Outer, calls
`0x4C7C60`, then removes twelve stack bytes. These are the same quiet/failable
flags produced by DynamicLoadObject(MayFail=true).
The current execDynamicLoadObject branch at `0x485A66` through `0x485AAC`
produces `0x2` for MayFail=false and `0x2002` for MayFail=true. The native
package fallback therefore uses the observed `0x2002`; `0x2` alone does not
include the failure-tolerant behavior selected by that parameter.

The routine obtains the package linker using `(Outer, Filename, flags|8, null,0)`.
Unless flag `0x10` is set, it calls `0x4A2100`, which iterates linker exports and
preloads classes/templates. It takes the linker root at `+0x3C`, finishes the
load, and validates the returned root against Core.Package at `0x44CFA0`.
The input is a borrowed UTF-16 filename pointer used during the synchronous call;
the routine does not take ownership of that caller buffer.

This provides a statically confirmed route for allowlisted cooked source files,
separate from object-path loading. It still requires live validation of each
package's dependencies, returned package identity, and resulting target object.
Load completion can invalidate cached arrays and metadata; refresh after both
success and failure. Do not infer that loading all exports activates a map or
makes its mission actors independent of the map's controller.

The preloader can pump Windows messages at `0x4A21C5` under engine flags. An
adapter calling it while holding shared state must reject reentrant callbacks
rather than blocking on the same mutex. Loading may take longer than the normal
item request; uncertain timeouts must never trigger an automatic repeated load
or spawn.

Evidence: `static_load_object.txt`, `resolve_name.txt`, `load_package.txt`,
`load_package_callers.txt`, and `load_package_0x4a2100.txt` under the ignored
analysis directory. No package-load call was executed in this investigation.

## Equipment subclass dispatch review

Current installed UDKGame.upk SHA-256 is
`5f532d3ecf74d85a74be3c6c36ca986a88da1573909ec8525474743479e1e824`.
Its decoded script confirms that CreateEquipmentFromNetInfo constructs the
selected template's actual class with that original archetype, then invokes
the new object's InitFromNetInfo. Subclass initialization is therefore part
of custom construction and the later dropped-item reconstruction.

The unfiltered random-roll route also uses subclass dispatch: the manager's
ServerCreateDroppedEquipment calls the dropped actor's InitMyEquipmentObject,
which invokes AddRandomizeValues on its newly constructed MyEquipmentObject.
The adapter's filtered route resolves AddRandomizeValues through the actual
class hierarchy before dispatch. Both routes must reject templates that disable
randomization; otherwise the ordinary manager route can return an item without
the requested randomization. These observations establish call selection, not
live success, save/reload behavior, or support for every catalog entry.

The current decoded call-chain evidence is retained locally in
`target/item-implementation/scripts/current-spawn-constructor.txt`.
