# Block containers: behavioural specification

Version: see `assets/minecraft/version.json`. Wire packet IDs: `assets/mcrs/reports/packets.json`.

Out of scope: menu clicks (`docs/spec/inventory.md`), block drops (`docs/spec/block-breaking.md`), and loot tables (`docs/spec/container-loot.md`).

## 0. Conventions
- **Single chest**: one `chest`, `trapped_chest`, or **copper chest** with block state `type = single`.
- **Double chest**: two adjacent chest blocks paired as `right` and `left` halves in one 54-slot menu.
- **Copper chest**: one of the eight copper chest variants (`unaffected`, `exposed`, `weathered`, `oxidized`, unwaxed or waxed).
- **Viewer count**: the non-negative count of viewers who keep a container block open.
- **Obstruction**: a solid opaque neighbor block that blocks an open action on a chest, `ender_chest`, or `shulker_box`.
- Prefixes: `CTR-TYP` (menus, titles, sounds), `CTR-DBL` (double chests), `CTR-OBS`/`CTR-SHK`/`CTR-END` (access), `CTR-LID` (**viewer count**), `CTR-E`/`CTR-U` (experiments, unresolved).

## 1. Classification and feedback

### CTR-TYP-001: Container menus, slots, and titles
| Block | Menu Key | Slots | Total Menu Slots | Default Title Key |
| --- | --- | --- | --- | --- |
| `chest`, `trapped_chest`, **copper chest** (`single`) | `minecraft:generic_9x3` | 27 | 63 | `container.chest` |
| `chest` (`double`), `copper_chest` (`double`) | `minecraft:generic_9x6` | 54 | 90 | `container.chestDouble` |
| `minecraft:barrel` | `minecraft:generic_9x3` | 27 | 63 | `container.barrel` |
| `minecraft:shulker_box`, `minecraft:cyan_shulker_box` | `minecraft:shulker_box` | 27 | 63 | `container.shulkerBox` |
| `minecraft:ender_chest` | `minecraft:generic_9x3` | 27 | 63 | `container.enderchest` |
| `minecraft:dispenser` / `minecraft:dropper` | `minecraft:generic_3x3` | 9 | 45 | `container.dispenser` / `container.dropper` |
| `minecraft:hopper` | `minecraft:hopper` | 5 | 41 | `container.hopper` |

Slots `0 .. Slots - 1` address the container, and the last 36 slots address the viewer inventory and hotbar.
Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`. Experiment: `CTR-E1`.

### CTR-TYP-002: Custom container title
When a container block entity carries a custom name component, `open_screen` must send that custom text instead of the default key.
Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`. Experiment: `CTR-E1`.

### CTR-TYP-003: Lid sound pairs and origin
Containers with lids emit `block` sounds (`source = 4`, volume `0.5`) from the block center, **double chest** midpoint, or barrel front. The barrel front sits at `center + 0.5 * facing` blocks, and sound packets scale coordinates by `8`.
| Container | Open Sound | Close Sound | Origin |
| --- | --- | --- | --- |
| `chest`, `trapped_chest` | `minecraft:block.chest.open` | `minecraft:block.chest.close` | Center or midpoint |
| **Copper chest** (`unaffected`, `exposed`, unwaxed or waxed) | `minecraft:block.copper_chest.open` | `minecraft:block.copper_chest.close` | Center or midpoint |
| **Copper chest** (`weathered`, unwaxed or waxed) | `minecraft:block.copper_chest_weathered.open` | `minecraft:block.copper_chest_weathered.close` | Center or midpoint |
| **Copper chest** (`oxidized`, unwaxed or waxed) | `minecraft:block.copper_chest_oxidized.open` | `minecraft:block.copper_chest_oxidized.close` | Center or midpoint |
| `barrel` | `minecraft:block.barrel.open` | `minecraft:block.barrel.close` | Front face center |
| `shulker_box`, `cyan_shulker_box` | `minecraft:block.shulker_box.open` | `minecraft:block.shulker_box.close` | Block center |
| `ender_chest` | `minecraft:block.ender_chest.open` | `minecraft:block.ender_chest.close` | Block center |
| `dispenser`, `dropper`, `hopper` | None | None | None |

Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`. Experiment: `CTR-E1`.

## 2. Double chests and access rules

### CTR-DBL-001: Pairing and slots for double chests
- Two adjacent `chest` or `copper_chest` blocks with identical horizontal `facing` pair as `left` and `right` halves.
- Adjacent `chest` + `trapped_chest`, `copper_chest` + `exposed_copper_chest`, and `copper_chest` + `waxed_copper_chest` must not pair.
- From `left` at `P`, `right` sits one block clockwise from `facing` (`north`: `left = -X`, `right = +X`). Slots `0..26` map to `right`, and `27..53` map to `left`.
- Both halves receive `block_event`, and the midpoint plays the sound. When a player breaks one half, the other half reverts to `type = single` with items intact.
Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`. Experiment: `CTR-E1`.

### CTR-OBS-001: Top obstruction and unobstructed containers
- Chests, `trapped_chest`, **copper chests**, and `ender_chest` check `y + 1` above (`minecraft:stone` blocks, `minecraft:glass` permits). On a **double chest**, an **obstruction** above either half refuses the open action from both halves.
- `barrel`, `dispenser`, `dropper`, and `hopper` run no **obstruction** check even under `minecraft:stone`.
Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`. Experiment: `CTR-E1`.

### CTR-OBS-002: Shulker box facing obstruction
A `shulker_box` must check `pos + facing` instead of `y + 1`. A solid opaque block at `pos + facing` stops the open action.
Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`. Experiment: `CTR-E1`.

### CTR-SHK-001: Shulker box item rules and storage
A `shulker_box` must reject any shulker box item inserted via `quick_move` or `pickup`, while a `chest` must permit it. A survival break stores slots in `minecraft:container`, and placement restores them.
Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`. Experiment: `CTR-E1`.

### CTR-END-001: Ender chest private inventory
An `ender_chest` must expose the private 27-slot `minecraft:generic_9x3` inventory of each viewer while viewers share the block **viewer count**.
Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`. Experiment: `CTR-E1`.

### CTR-LID-001: Viewer count and lid feedback
Chests, `ender_chest`, `shulker_box`, and `cyan_shulker_box` track **viewer count** (`0`, `1`, `2` probed) with `0 -> 1` open and `1 -> 0` close sounds. Each open or close sends `block_event` (`action = 1`, `param = viewer count`). A `barrel` updates `open` (`true` when nonzero, `false` at `0`) with no `block_event`, while `dispenser`, `dropper`, and `hopper` emit neither.
Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`. Experiment: `CTR-E1`.

### CTR-LID-002: Periodic chest block-event refresh
While **viewer count** is nonzero, chests and `ender_chest` re-emit `block_event` (`action = 1`) every `5` ticks. They also send a second `param = 0` on `1 -> 0` close, whereas `shulker_box` and `cyan_shulker_box` omit both.
Provenance: `black_box_confirmed`. Kind: `quirk`. Confidence: `high`. Experiment: `CTR-E1`.

## 3. Known vanilla quirks
- **Rule**: `CTR-LID-002`. Chests and `ender_chest` send 5-tick `block_event` refreshes and a second `param = 0` after close. Shulker boxes send only `1` and `0`.
- **Observer**: the vanilla client lid animation on chests and `ender_chest`, which drifts without periodic and close updates.
- **Decision**: match this cadence on chests and `ender_chest` for client lid parity. **Bug reference**: none.

## 4. Experiments
- **CTR-E1** (`tools/mcrs_minecraft_probe/scenarios/containers.json` -> `tools/mcrs_minecraft_probe/traces/containers.json`):
  1. Connect `Alice` and `Bob`; test `chest` (300 ms wait, `stone`, `glass`, custom title, shulker insert) and `barrel`.
  2. Test double `chest`, `trapped_chest`, half-broken double `chest`, **copper chests**, `shulker_box`, `cyan_shulker_box`, `ender_chest` (300 ms wait), and `dispenser`/`dropper`/`hopper` under `stone`.
  - Result: `open_screen`, `sounds`, `block_events`, `open` state, and `minecraft:container` data match `CTR-TYP-001` through `CTR-LID-002`.

## 5. Unresolved
- **CTR-U1**: The other 15 dyed `shulker_box` variants are not probed beyond `shulker_box` and `cyan_shulker_box`.
- **CTR-U2**: `block_event` byte cap (`min(viewer count, 255)`) above 255 viewers is not probed in `CTR-E1`.
- **CTR-U3**: Double `trapped_chest`, double oxidized or waxed **copper chests**, and mismatched horizontal facings are not probed in `CTR-E1`.
