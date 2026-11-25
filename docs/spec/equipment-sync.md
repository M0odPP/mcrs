# Player equipment sync: behavioural specification

Version: see `assets/minecraft/version.json`. Wire packet IDs: `assets/mcrs/reports/packets.json`.

This document states how a server sends player equipment to **viewers** and when equipment actions
emit **equip sounds**.

Out of scope:
- entity tracker range and lifecycle of a spawn bundle (`entity-tracking.md`);
- client storage and entity visuals (`client-entities.md`, `CENTS-EQUIP-001`; `client-entity-render.md`);
- non-player entities, `body` (`6`), and `saddle` (`7`) equipment (`mob-animals.md`, `ANIM-INT-007`);
- death drops and respawn inventory reset (`player-death.md`);
- self-inventory menu sync (`inventory.md`, `INV-SYN-003`, `INV-SYN-005`).

## 0. Conventions

### 0.1 Terms

- **Actor**: a connected player whose **equipment slots** change or appear to another player.
- **Viewer**: a connected player who tracks **actor** under `entity-tracking.md`. A player never
  appears in the viewer set of that player.
- **Equipment slot**: one of the six slots `mainhand`, `offhand`, `feet`, `legs`, `chest`, and
  `head` of a player.
- **Equipment packet**: the clientbound packet `minecraft:set_equipment` (`104`).
- **Swap event**: the clientbound packet `minecraft:entity_event` (`34`) with status `55`.
- **Equip sound**: a clientbound packet `minecraft:sound` (`119`) from the `equip_sound` field of
  `minecraft:equippable` (`item-stack.md`, `STK-SHP-028`).
- **Wire stack**: an item stack encoded in the wire format of `item-stack.md` (`STK-NET-001`).

### 0.2 Provenance and kinds

Rule tags follow `registry.md` and `docs/spec/README.md` (`black_box_confirmed` or `design`; kind
`wire` or `semantic`; confidence `high`). Rules rely on experiments against a vanilla server of the
version in `assets/minecraft/version.json` and on
`tools/mcrs_minecraft_probe/traces/equipment_sync.json`.

### 0.3 ID prefixes

- `EQP-PKT-*`: wire format of `minecraft:set_equipment` (`104`) and `minecraft:entity_event` (`34`).
- `EQP-SLT-*`: **equipment slots** of a player and inventory sources.
- `EQP-SYN-*`: spawn sync, live updates, self-exclusion, broadcast-once bytes, and hand swaps.
- `EQP-SND-*`: **equip sounds**.
- `EQP-E*`: verification experiments.

### 0.4 Artefacts cited

- `assets/mcrs/reports/packets.json`
- `tools/mcrs_minecraft_probe/scenarios/equipment_sync.json`
- `tools/mcrs_minecraft_probe/traces/equipment_sync.json`

---

## 1. Wire packets

### EQP-PKT-001: clientbound `minecraft:set_equipment` (`104`)

The **equipment packet** (`play`, clientbound ID `104`) encodes `entity_id` (`VarInt`) and a
non-empty list of `(slot_byte, stack)` entries (`client-entities.md`, `CENTS-EQUIP-001`). Bits
`0` to `6` of `slot_byte` store the slot ID (`0` to `7`). Bit `7` (`0x80`) of `slot_byte` must
be `1` on every entry except the last entry, where bit `7` must be `0`.

Entries in one packet must appear in slot ID order from lowest to highest. Each `stack` is a
**wire stack**; an empty **wire stack** clears that slot on the client (`equip_helmet_by_use`,
`use_helmet_equip`). The server must send zero **equipment packets** when no entry belongs in the
packet.

Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`.
Experiment: `EQP-E1` through `EQP-E5`; `tools/mcrs_minecraft_probe/traces/equipment_sync.json`
(`set_hotbar_slots`, `equip_armour_via_console`, `equip_helmet_by_use`, `use_helmet_equip`,
`join_charlie_observer`).

### EQP-PKT-002: clientbound `minecraft:entity_event` (`34`) swap status

The **swap event** (`play`, clientbound ID `34`) encodes `entity_id` (32-bit signed integer) and
`entity_status` (signed byte). For an offhand swap (`EQP-SYN-005`), `entity_id` must be the network
ID of **actor**, and `entity_status` must be `55`.

Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`.
Experiment: `EQP-E2`; `tools/mcrs_minecraft_probe/traces/equipment_sync.json` (`swap_offhand`).

---

## 2. Player equipment slots

### EQP-SLT-001: six equipment slots of a player

A player has six **equipment slots** that map to the player inventory (`inventory.md`,
`INV-SLT-001`, `INV-SEL-001`):

| Slot ID | Name | Inventory source |
|---------|------|------------------|
| `0` | `mainhand` | selected hotbar slot `P[n]`, `n` in `0..8` |
| `1` | `offhand` | `P[40]` |
| `2` | `feet` | `P[36]` |
| `3` | `legs` | `P[37]` |
| `4` | `chest` | `P[38]` |
| `5` | `head` | `P[39]` |

Changes to an unselected hotbar slot (`set_hotbar_slots`) or to slots `P[9]` through `P[35]`
(`set_backpack_slot`) must not emit an **equipment packet**. A change of the selected hotbar slot
(`INV-SEL-002`) must re-evaluate `mainhand`.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `EQP-E1`, `EQP-E2`, `EQP-E3`, `EQP-E5`;
`tools/mcrs_minecraft_probe/traces/equipment_sync.json` (`set_hotbar_slots`, `set_backpack_slot`,
`select_slot_1`, `select_slot_0`, `equip_armour_via_console`, `join_charlie_observer`).

---

## 3. Spawn and live sync

### EQP-SYN-001: spawn sync for a new viewer

When a **viewer** starts to track **actor**, the server checks the **equipment slots** of **actor**.
If at least one slot is non-empty, the server must send one **equipment packet** for **actor** to
that **viewer**. That spawn packet must contain only non-empty **equipment slots** in slot ID order
(`0` to `5`).

When all six **equipment slots** of **actor** are empty, the server must send zero **equipment
packets** to that **viewer**. When a new **viewer** starts to track **actor**, an earlier
**viewer** must receive zero repeat **equipment packets**.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `EQP-E5`; `tools/mcrs_minecraft_probe/traces/equipment_sync.json` (`join_alice`,
`join_bob`, `join_charlie_observer`).

### EQP-SYN-002: live updates on slot, selection, and item-use changes

Each tick, the server compares the six **equipment slots** of **actor** against the last state sent
for **actor**. A slot differs when its item kind, count (`set_count_only`), or component patch
(`set_patch_only`) differs (`item-stack.md`, `STK-EQ-002`).

When at least one slot differs, the server must send one **equipment packet** to each **viewer** of
**actor**, except where `EQP-SYN-005` applies. That packet must contain every slot that differs.
Multiple slot changes for **actor** in one tick must coalesce into one **equipment packet**. An
emptied slot must appear as an empty **wire stack** (`EQP-PKT-001`).

When an item use equips armour and emits an arm swing, each **viewer** sees `swing_animation`
before `set_equipment` (`arm-swing.md`, `SWG-DST-003`).

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `EQP-E1`, `EQP-E3`, `EQP-E4`; `tools/mcrs_minecraft_probe/traces/equipment_sync.json`
(`set_hotbar_slots`, `set_count_only`, `set_patch_only`, `select_slot_1`, `select_slot_0`,
`equip_armour_via_console`, `equip_helmet_by_use`, `use_helmet_equip`).

### EQP-SYN-003: self-exclusion on equipment packets and swap events

The server must never send an **equipment packet** or a **swap event** for **actor** to **actor**
itself. **Actor** updates its own view of its slots from client prediction or from self-inventory
packets (`inventory.md`, `INV-SYN-003`, `INV-SYN-005`). When server inventory state or item-use
state differs from client prediction, self sync uses `container_set_slot` or `container_set_content`,
not `set_equipment`.

Provenance: `black_box_confirmed` for self-exclusion; `inventory.md` (`INV-SYN-003`, `INV-SYN-005`)
for self sync. Kind: `semantic`. Confidence: `high`.
Experiment: `EQP-E1` through `EQP-E4`; `tools/mcrs_minecraft_probe/traces/equipment_sync.json`
(`set_hotbar_slots`, `set_count_only`, `set_patch_only`, `select_slot_1`, `select_slot_0`,
`swap_offhand`, `equip_armour_via_console`, `equip_helmet_by_use`, `use_helmet_equip`).

### EQP-SYN-004: identical payload bytes across viewers

Every **viewer** of **actor** in one tick must receive identical payload bytes for one live
**equipment packet** (`EQP-PKT-001`) or one **swap event** (`EQP-PKT-002`).

Provenance: `design`. Kind: `wire`. Confidence: `high`.
Reason: packet fields depend only on **actor** and not on the **viewer**.

### EQP-SYN-005: offhand swap emits entity event `55`

Suppose `player_action` `swap_item_with_offhand` (`7`) exchanges `mainhand` and `offhand` in one
tick. The server must keep both updated stacks for future spawn sync (`EQP-SYN-001`). For that
exchange, the server must send one **swap event** (`EQP-PKT-002`) to each **viewer** of **actor**.
The server must send zero **equipment packets** for `mainhand` or `offhand`.

When `mainhand` and `head` both change without a swap of `mainhand` and `offhand`, the server must
send an **equipment packet** (`EQP-SYN-002`) and zero **swap events**.

Provenance: `black_box_confirmed`. Kind: `wire` and `semantic`. Confidence: `high`.
Experiment: `EQP-E2`, `EQP-E4`, `EQP-E5`; `tools/mcrs_minecraft_probe/traces/equipment_sync.json`
(`swap_offhand`, `equip_helmet_by_use`, `use_helmet_equip`, `join_charlie_observer`).

---

## 4. Equip sounds

### EQP-SND-001: equip sound on armour equip

When slot replacement or item use equips an item in `feet`, `legs`, `chest`, or `head`, the server
emits `minecraft:sound` (`119`) for `equip_sound` (`item-stack.md`, `STK-SHP-028`). Both **actor**
and nearby **viewers** receive that **equip sound** with category `7` (`player`), volume `1.0`,
and pitch `1.0`. The packet carries a shared `seed` and the position of **actor** in eighths of a
block.

In `equip_armour_via_console` and `use_helmet_equip`, **actor** and nearby **viewers** receive the
**equip sound** packets before `set_equipment`. An empty armour slot, a hotbar replacement, a hotbar
selection, and an offhand swap each emit zero **equip sounds**.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `EQP-E1` through `EQP-E4`; `tools/mcrs_minecraft_probe/traces/equipment_sync.json`
(`set_hotbar_slots`, `set_backpack_slot`, `set_count_only`, `set_patch_only`, `select_slot_1`,
`select_slot_0`, `swap_offhand`, `equip_armour_via_console`, `equip_helmet_by_use`,
`use_helmet_equip`).

---

## 5. Experiments

Each experiment runs against the vanilla server (`assets/minecraft/version.json`) with
`tools/mcrs_minecraft_probe/scenarios/equipment_sync.json` and records
`tools/mcrs_minecraft_probe/traces/equipment_sync.json`.

- **EQP-E1** (`hotbar`, backpack, count/patch updates, and selection):
  1. Join `Alice` and `Bob`, then set both players to `survival` mode.
  2. Clear both players and teleport them near `(0.5, -60, 0.5)`.
  3. Replace `Alice` `hotbar.0` with `diamond_sword` and `hotbar.1` with `iron_pickaxe` (`set_hotbar_slots`).
  4. Replace `Alice` `inventory.0` (`P[9]`) with `emerald` (`set_backpack_slot`).
  5. Set `Alice` `hotbar.0` to `apple` `1` (`set_count_base`), then `apple` `2` (`set_count_only`).
  6. Set `Alice` `hotbar.0` to `diamond_sword[damage=5]` (`set_patch_base`), then `damage=10` (`set_patch_only`).
  7. Restore `hotbar.0` to `diamond_sword`, select slot `1` (`select_slot_1`), then select slot `0` (`select_slot_0`).
  - Result: `join_alice`, `join_bob`, and `set_backpack_slot` emit zero `set_equipment` packets.
    `set_hotbar_slots`, `set_count_only`, `set_patch_only`, `select_slot_1`, and `select_slot_0`
    each send one `set_equipment` with `mainhand` only to `Bob`, zero to `Alice`, and zero
    `sound` packets.
- **EQP-E2** (`offhand` swap):
  1. Send `player_action` `swap_item_with_offhand` from `Alice` (`swap_offhand`).
  2. Wait `200` ms (`wait_after_swap`).
  - Result: `Bob` receives one `entity_event` with status `55` for `Alice` and zero
    `set_equipment` or `sound` packets. `Alice` receives zero `entity_event` and zero
    `set_equipment` packets.
- **EQP-E3** (armour slots via console):
  1. Replace `Alice` `armor.head`, `armor.chest`, `armor.legs`, and `armor.feet` with `iron_helmet`,
     `diamond_chestplate`, `chainmail_leggings`, and `leather_boots` (`equip_armour_via_console`).
  - Result: `Alice` and `Bob` both receive `minecraft:item.armor.equip_iron`,
    `minecraft:item.armor.equip_diamond`, `minecraft:item.armor.equip_chain`, and
    `minecraft:item.armor.equip_leather`. `Bob` alone then receives one `set_equipment` with
    `feet`, `legs`, `chest`, and `head` in slot ID order.
- **EQP-E4** (helmet equip via `use_item`):
  1. Clear `Alice` `armor.head` to `air` and place `golden_helmet` in `hotbar.0` (`equip_helmet_by_use`).
  2. Send `use_item` on `main` from `Alice` (`use_helmet_equip`).
  - Result: `equip_helmet_by_use` sends `set_equipment` (`mainhand` = `golden_helmet`, `head` = empty)
    only to `Bob`, with zero sounds. `use_helmet_equip` sends `minecraft:item.armor.equip_gold` to
    `Alice` and `Bob`, then `swing_animation` and `set_equipment` (`mainhand` = empty, `head` =
    `golden_helmet`) only to `Bob`.
- **EQP-E5** (late **viewer** spawn):
  1. Join `Charlie` (`join_charlie_observer`).
  2. Set `Charlie` to `survival` mode and teleport `Charlie` near `Alice` (`move_charlie_near`, `wait_charlie`).
  - Result: `Charlie` receives one `set_equipment` for `Alice` with `offhand`, `feet`, `legs`,
    `chest`, and `head` in slot ID order. That packet omits the empty `mainhand` slot, and `Bob` and
    `Alice` receive zero `set_equipment` packets.

---

## 6. Unresolved

None.
