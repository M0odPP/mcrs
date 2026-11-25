# Item use: behavioural specification

Version: see `assets/minecraft/version.json`. Wire packet IDs: `assets/mcrs/reports/packets.json`. Item
component IDs: `assets/mcrs/reports/registries.json`. Item definitions:
`assets/mcrs/item_definition/`.

This document states when a server starts, updates, stops, and cancels continuous item use for an
**actor**. It states the wire format of **living flags**, the movement speed factor, and metadata
recipients.

Out of scope:
- entity trackers (`entity-tracking.md`);
- instant-use arm swings (`arm-swing.md`);
- block placement and block interaction (`block-placing.md`);
- food nutrition, saturation, and exhaustion (`hunger.md`);
- status effect calculation (`status-effects.md`);
- shield damage reduction and shield cooldown (`entity-damage.md`, `DMG-HIT-005`).

## 0. Conventions

### 0.1 Terms

- **Actor**: a player who sends an item-use trigger or holds an **active use**.
- **Viewer**: a connected player who tracks **actor** under `entity-tracking.md`, never equal to
  **actor**.
- **Hand**: an item slot selector on the wire, either `main` (`0`) or `off` (`1`).
- **Active use**: the continuous item-use state of **actor** for one **hand** and one item stack.
- **Use duration**: the tick count limit of an **active use** in ticks (`use_duration_ticks`).
- **Ticks in use**: the count of server ticks elapsed since an **active use** started.
- **Living flags**: the byte entry at metadata index `8` of `minecraft:player` (`entity_types.json`).
- **Usable item**: an item stack with a data component that starts continuous use, such as
  `minecraft:blocks_attacks` or `minecraft:consumable`.
- **Loaded client**: a connected player after `player_loaded` or `60` ticks after join or respawn
  (`block-placing.md`, `PLC-REQ-005`).

### 0.2 Provenance and kinds

Rule labels follow `registry.md`, section 0.3 (`black_box_confirmed`, `documentation`, `design`,
or `unresolved`). Rule kinds follow `docs/spec/README.md`, section "Semantics, not accidents". Every
rule rests on captured game artefacts, public documentation, or explicit design choices.

### 0.3 ID prefixes

- `USE-PKT-*`: serverbound and clientbound packets and **living flags** bit layout.
- `USE-STR-*`: start preconditions, mode rules, and **use duration** setup.
- `USE-TCK-*`: hold tick progression, speed factor, and natural finish.
- `USE-END-*`: explicit release, hotbar slot cancel, and invalidation stops.
- `USE-NET-*`: self and **viewer** metadata delivery.
- `USE-EDN-*`: edition-neutral state contract.
- `USE-E*`: verification experiments.
- `USE-U*`: unresolved items.

### 0.4 Artefacts cited

- `assets/mcrs/reports/packets.json`
- `assets/mcrs/reports/registries.json`
- `assets/mcrs/item_definition/shield.json`, `apple.json`, `golden_apple.json`, `bow.json`,
  `stick.json`, `wooden_spear.json`
- `crates/mcrs_minecraft_model/tests/fixtures/vanilla/entity_types.json`
- `crates/mcrs_minecraft_physics/tests/fixtures/player/e23_sprint_while_using.txt`
- `tools/mcrs_minecraft_probe/scenarios/item_use.json`
- `tools/mcrs_minecraft_probe/traces/item_use.json`
- `tools/mcrs_minecraft_probe/traces/arm_swing.json`

---

## 1. Wire packets and metadata

### USE-PKT-001: serverbound `minecraft:use_item` (`67`)

`minecraft:use_item` (`play`, serverbound ID `67`) carries four fields in order:

| Field | Wire type | Values |
|-------|-----------|--------|
| `hand` | `VarInt` | `0` = `main`, `1` = `off` |
| `sequence` | `VarInt` | Non-negative prediction sequence |
| `yaw` | `Float` | Look yaw in degrees |
| `pitch` | `Float` | Look pitch in degrees |

Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`.
Experiment: `USE-E1`; `assets/mcrs/reports/packets.json`;
`tools/mcrs_minecraft_probe/traces/item_use.json` (`start_use_main_shield`, `use_offhand_shield`,
`use_golden_apple_always_eat`, `use_shield_adventure`).

### USE-PKT-002: serverbound stop packets (`41`, `54`)

Two serverbound play packets stop an **active use**:

1. `minecraft:player_action` (`41`) with status `release_use_item` (`6`).
2. `minecraft:set_carried_item` (`54`) with a new hotbar slot (`0` to `8`).

Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`.
Experiment: `USE-E1`; `assets/mcrs/reports/packets.json`;
`tools/mcrs_minecraft_probe/traces/item_use.json` (`release_use_shield`,
`release_offhand_shield`, `switch_slot_cancels_use`, `cancel_gapple_via_slot`, `release_adv`).

### USE-PKT-003: clientbound `minecraft:set_entity_data` (`101`) index `8` (**living flags**)

The server sends **living flags** at metadata index `8` (`wire_type` `0`, byte, default `0`) in
`minecraft:set_entity_data` (`101`):

| Bit | Mask | Definition |
|-----|------|------------|
| `0` | `0x01` | `1` when **active use** is active, `0` when inactive |
| `1` | `0x02` | `0` for `main` **hand**, `1` for `off` **hand** |
| `2` | `0x04` | `1` for riptide spin attack, `0` otherwise (`player-pose.md`, `POSE-NET-002`) |

Join metadata omits index `8` while its value equals `0`.
When `main` use starts, the server clears bit `1` and sets value `1`.
When `off` use starts, the server sets bits `0` and `1` (value `3`).
When `off` use stops, the server clears bit `0` and keeps bit `1` (value `2`) until `main` use resets bit `1`.

Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`.
Experiment: `USE-E1`; `crates/mcrs_minecraft_model/tests/fixtures/vanilla/entity_types.json`;
`player-pose.md` (`POSE-NET-002`); `tools/mcrs_minecraft_probe/traces/item_use.json` (`join_alice`,
`join_bob`, `start_use_main_shield`, `release_use_shield`, `use_offhand_shield`,
`release_offhand_shield`, `cancel_gapple_via_slot`, `use_golden_apple_always_eat`,
`creative_eat_and_release`, `use_shield_adventure`, `release_adv`).

---

## 2. Preconditions and start

### USE-STR-001: item type and game mode rules

In `survival`, `creative`, and `adventure` modes, `use_item` starts an **active use** when **hand**
holds a **usable item** that meets `USE-STR-002`:

1. Items with `minecraft:blocks_attacks` (`minecraft:shield`) start an **active use** in `survival`
   (both `main` and `off`) and `adventure` (`main`).
2. Inert items (`minecraft:stick`) start no **active use**. They emit zero `set_entity_data` or
   `swing_animation` packets.
3. Empty hands and instant-use items (`minecraft:snowball`, empty `minecraft:map`,
   `minecraft:fishing_rod`) start no **active use** (`arm-swing.md`, `SWG-SRC-005`).

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`, `USE-E3`; `assets/mcrs/item_definition/shield.json`, `stick.json`;
`tools/mcrs_minecraft_probe/traces/item_use.json` (`start_use_main_shield`, `use_offhand_shield`,
`try_use_stick_inert`, `use_shield_adventure`); `tools/mcrs_minecraft_probe/traces/arm_swing.json`
(`use_item_snowball_offhand`, `use_item_empty_map_observers_only`, `use_item_empty_no_swing`).

### USE-STR-002: consumable gate and **use duration**

When **hand** holds an item with `minecraft:consumable`:

1. At food level `20` in `survival` mode, `minecraft:apple` (`can_always_eat` `false`) starts no
   **active use**. It emits zero packets.
2. In `creative` mode (`minecraft:apple`) or when `can_always_eat` is `true`
   (`minecraft:golden_apple`), `use_item` starts at food level `20`.
3. For `minecraft:consumable`, **use duration** (`use_duration_ticks`) equals
   `trunc(consume_seconds * 20)` (`32` ticks at default `1.6`).
4. When `use_duration_ticks` equals `0`, the consume finishes at start with zero **active use**
   (`hunger.md`, `HNG-EAT-003`).
5. When `food < 20` or the item lacks `minecraft:food`, a `minecraft:consumable` item starts
   (`hunger.md`, `HNG-EAT-002`).
6. For `minecraft:blocks_attacks`, **active use** stays open until release or cancel.

Provenance: items 1, 2, 6 `black_box_confirmed` (`assets/mcrs/item_definition/apple.json`,
`golden_apple.json`, `shield.json`; `tools/mcrs_minecraft_probe/traces/item_use.json`:
`try_apple_at_full_food`, `use_golden_apple_always_eat`, `creative_eat_and_release`,
`start_use_main_shield`); items 3, 4, 5 `documentation` (`hunger.md`, `HNG-EAT-002`,
`HNG-EAT-003`; see `USE-U1`). Kind: `semantic`. Confidence: `high`.

### USE-STR-003: start state and zero arm swing

At start, the server sets bit `0` of **living flags** to `1` and sets bit `1` to match **hand**.
It sends `set_entity_data` to **actor** and each **viewer**.
It emits zero `swing_animation` packets (`arm-swing.md`, `SWG-SRC-005`).

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`, `USE-E3`; `tools/mcrs_minecraft_probe/traces/item_use.json`
(`start_use_main_shield`, `use_offhand_shield`, `use_golden_apple_always_eat`,
`creative_eat_and_release`, `use_shield_adventure`);
`tools/mcrs_minecraft_probe/traces/arm_swing.json` (`use_item_eat_start_no_swing`,
`use_item_bow_no_swing`, `use_item_shield_no_swing`).

### USE-STR-004: client load, life, `spectator`, and re-trigger gate

The server must ignore `use_item` when **actor** is not a **loaded client**, is dead, is in
`spectator` mode, or already holds an **active use**.

Provenance: `design` (matches `block-placing.md` `PLC-REQ-005` and `arm-swing.md` `SWG-SRC-001`;
see `USE-U2`). Kind: `semantic`. Confidence: `high`.

---

## 3. Hold ticks, movement speed, and completion

### USE-TCK-001: hold tick progression

While an **active use** continues and **ticks in use** stays below **use duration**, the server
increases **ticks in use** by `1` each server tick.
The server sends zero extra **living flags** packets in hold ticks.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`; `tools/mcrs_minecraft_probe/traces/item_use.json` (`hold_3_ticks`,
`wait_before_slot_cancel`, `wait_offhand`, `wait_adv`).

### USE-TCK-002: movement speed multiplier (`0.2`)

On foot with `minecraft:bow` in **active use**, the movement step multiplies forward and sideways
input by `0.2` and stops sprint starts (`entity-physics.md`, `PHY-INP-001`).
For general items on a non-passenger **actor**, the movement step reads `speed_multiplier` and
`can_sprint` from `minecraft:use_effects` (defaults `0.2` and `false`; `PHY-INP-019`).

Provenance: `black_box_confirmed` for `minecraft:bow` on foot (`USE-E2`;
`crates/mcrs_minecraft_physics/tests/fixtures/player/e23_sprint_while_using.txt`;
`assets/mcrs/item_definition/bow.json`); `documentation` for custom `minecraft:use_effects` and
passenger exemption (`entity-physics.md`, `PHY-INP-019`; see `USE-U3`). Kind: `client agreement`.
Confidence: `high` (`bow`), `medium` (custom `use_effects`).

### USE-TCK-003: consumable natural finish

When **ticks in use** for `minecraft:golden_apple` (`1.6` s default) reaches **use duration**:

1. **Viewers** receive `minecraft:entity.generic.eat` sounds and then `minecraft:entity.player.burp`
   (volume `0.5`).
2. The server sends `entity_event` (`status` `9`) to **actor**.
3. The server reduces stack count by `1`. It sends `container_set_slot` to **actor** and
   `set_equipment` to **viewers**.
4. The server applies `on_consume_effects` (`regeneration` and `absorption`). It updates metadata
   index `10` effect particles and index `17` float `4.0`.
5. The server clears bit `0` of **living flags** (`0` for `main`) and sends `set_entity_data` to
   **actor** and **viewers**.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`; `assets/mcrs/item_definition/golden_apple.json`;
`tools/mcrs_minecraft_probe/traces/item_use.json` (`use_golden_apple_always_eat`,
`wait_consume_finish`).

---

## 4. Release and cancellation

### USE-END-001: release on `release_use_item`

When **actor** sends `player_action` with status `release_use_item` (`6`) while in **active use**,
the server must clear **active use**.
It clears bit `0` of **living flags** while bit `1` stays at the **hand** bit (`0` for `main`, `2`
for `off`).
The server sends only `set_entity_data` to **actor** and each **viewer** with zero `sound` packets.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`; `tools/mcrs_minecraft_probe/traces/item_use.json` (`release_use_shield`,
`release_offhand_shield`, `creative_eat_and_release`, `release_adv`).

### USE-END-002: cancel on hotbar slot change

When **actor** sends `set_carried_item` (`54`) to a different hotbar slot while in `main` **active
use**, the server must cancel **active use**.
It clears bit `0` of **living flags**, sends `set_entity_data` to **actor** and each **viewer**,
and sends `set_equipment` to each **viewer**.
A cancelled consumable keeps its stack unchanged and applies zero consume effects or sounds.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`; `tools/mcrs_minecraft_probe/traces/item_use.json`
(`start_use_main_then_swap_slot`, `wait_before_slot_cancel`, `switch_slot_cancels_use`,
`cancel_gapple_via_slot`, `select_gapple_finish`).

### USE-END-003: death and stack invalidation

When **actor** dies or enters `spectator` mode, the server clears **active use** and bit `0` of
**living flags**.
When the used stack becomes empty or changes item type, the server clears **active use** and bit
`0` of **living flags**.

Provenance: `design` (see `USE-U2`). Kind: `semantic`. Confidence: `high`.

---

## 5. Metadata recipients and edition-neutral contract

### USE-NET-001: self, observer delivery, and equipment order

Every change of **living flags** sends `set_entity_data` index `8` to **actor** and to each
**viewer** in the same dimension (`client-knowledge.md`, `CK-SET-001`; `movement-checks.md`,
`MVC-PKT-031`).
At start, **viewers** receive `set_equipment` before `set_entity_data`.
At hotbar slot cancel, **viewers** receive `set_entity_data` before `set_equipment`.
Tracker range and dimension filters follow `entity-tracking.md`.

Provenance: `black_box_confirmed` for **actor**, **viewer** (`Alice` and `Bob`), and packet order;
`design` for tracker range and dimension filter (`entity-tracking.md`; see `USE-U4`). Kind: `wire`,
`semantic`. Confidence: `high`.
Experiment: `USE-E1`; `tools/mcrs_minecraft_probe/traces/item_use.json` (`start_use_main_shield`,
`release_use_shield`, `switch_slot_cancels_use`, `use_offhand_shield`, `release_offhand_shield`,
`cancel_gapple_via_slot`, `use_golden_apple_always_eat`, `creative_eat_and_release`,
`use_shield_adventure`, `release_adv`).

### USE-EDN-001: edition-neutral rules boundary

Core item-use rules track only **hand**, **use duration** (`use_duration_ticks`),
**ticks in use**, `minecraft:use_effects`, `minecraft:consumable`, and `minecraft:blocks_attacks`.
Wire adapters map Java packets (`use_item`, `player_action`, `set_carried_item`, `set_entity_data`)
or Bedrock input actions onto those rules.

Provenance: `design`. Kind: `semantic`. Confidence: `high`.

---

## 6. Intentional deviations

None.

---

## 7. Known vanilla quirks

None.

---

## 8. Experiments

- **USE-E1** (`tools/mcrs_minecraft_probe/scenarios/item_use.json` ->
  `tools/mcrs_minecraft_probe/traces/item_use.json`):
  1. Connect `Alice` and `Bob` in `survival` mode (`join_alice`, `join_bob`).
  2. Equip `Alice` with `shield` (`main`), `apple` (`off`), and `stick` (`hotbar.1`).
  3. Send `use_item` (`main`) (`start_use_main_shield`).
  4. Wait `250` ms (`hold_3_ticks`).
  5. Send `release_use_item` (`release_use_shield`).
  6. Send `use_item` (`main`) (`start_use_main_then_swap_slot`).
  7. Wait `150` ms (`wait_before_slot_cancel`).
  8. Select hotbar slot `1` (`switch_slot_cancels_use`).
  9. Send `use_item` (`main`) with `stick` (`try_use_stick_inert`).
  10. Equip `shield` in `off` (`setup_offhand_shield`).
  11. Send `use_item` (`off`) (`use_offhand_shield`).
  12. Wait `150` ms (`wait_offhand`).
  13. Send `release_use_item` (`release_offhand_shield`).
  14. Equip `apple` in `hotbar.0` and `golden_apple` in `hotbar.2`.
  15. Select `apple` (`select_apple`).
  16. Send `use_item` (`main`) at food `20` (`try_apple_at_full_food`).
  17. Select `golden_apple` (`select_golden_apple`).
  18. Send `use_item` (`main`) (`cancel_gapple_via_slot`).
  19. Wait `200` ms inside `cancel_gapple_via_slot`.
  20. Select hotbar slot `1` inside `cancel_gapple_via_slot`.
  21. Select `golden_apple` (`select_gapple_finish`).
  22. Send `use_item` (`main`) (`use_golden_apple_always_eat`).
  23. Wait `1900` ms (`wait_consume_finish`).
  24. Set `creative` mode (`creative_eat_setup`).
  25. Select `apple` (`select_apple_creative`).
  26. Send `use_item` (`main`) (`creative_eat_and_release`).
  27. Wait `200` ms inside `creative_eat_and_release`.
  28. Send `release_use_item` inside `creative_eat_and_release`.
  29. Set `adventure` mode (`adventure_mode_setup`).
  30. Equip `shield` in `main` (`select_shield_adv`).
  31. Send `use_item` (`main`) (`use_shield_adventure`).
  32. Wait `200` ms (`wait_adv`).
  33. Send `release_use_item` (`release_adv`).
  - Result: `start_use_main_shield`, `creative_eat_and_release`, and `use_shield_adventure` send
    `index 8 = 1` (`Bob`: `set_equipment` then `set_entity_data`).
  - Result: wait steps emit zero metadata.
  - Result: `release_use_shield`, `release_adv`, `switch_slot_cancels_use`, and
    `cancel_gapple_via_slot` send `index 8 = 0`.
  - Result: slot cancel preserves `golden_apple` count `1`.
  - Result: `try_use_stick_inert` and `try_apple_at_full_food` emit zero packets.
  - Result: `use_offhand_shield` sends `index 8 = 3`, and `release_offhand_shield` sends `2`.
  - Result: `use_golden_apple_always_eat` sends `index 8 = 1`, `8` eat sounds, `1` burp sound, and
    `entity_event` `9`.
  - Result: at finish, the server sends slot `38` `null`, `index 8 = 0`, `index 10` particles, and
    `index 17` `4.0`.
- **USE-E2** (`crates/mcrs_minecraft_physics/tests/fixtures/player/e23_sprint_while_using.txt`):
  1. Start `use_item` with `bow`.
  2. Hold forward and sprint for two seconds.
  3. Send `release_use_item`.
  - Result: horizontal speed stays scaled by `0.2` without sprint until `release_use_item`.
- **USE-E3** (`tools/mcrs_minecraft_probe/traces/arm_swing.json`):
  1. Send `use_item` for `apple`, `bow`, `shield`, `snowball` (`off`), and empty hand.
  - Result: continuous start and release emit zero `swing_animation` packets. Empty hand starts
    nothing; instant uses swing without **active use**.

---

## 9. Unresolved

- **USE-U1** (`food < 20`, custom `consume_seconds`, and `use_duration_ticks` `0`):
  Experiment: probe `apple` at `food < 20` and a custom `consumable` with `consume_seconds: 0.0`.
- **USE-U2** (unloaded client, dead **actor**, `spectator`, re-trigger, and death cancel):
  Experiment: probe `use_item` and `/kill` with `entity_data` checks across those states.
- **USE-U3** (custom `use_effects` and passenger speed exemption):
  Experiment: record `wooden_spear` (`can_sprint: true`) and a boat passenger with `shield`.
- **USE-U4** (tracker range, dimension, and `spectator` viewer metadata):
  Experiment: verify `set_entity_data` index `8` delivery to `spectator`, distant, and nether
  viewers.
