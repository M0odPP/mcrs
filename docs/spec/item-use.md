# Item use: behavioural specification

Version: see `assets/minecraft/version.json`. Wire packet IDs: `assets/mcrs/reports/packets.json`. Item
component IDs: `assets/mcrs/reports/registries.json`. Item definitions:
`assets/mcrs/item_definition/`.

This document states when a server starts, updates, stops, and cancels continuous item use for a
player. It states the wire format of **living flags**, the movement speed factor, and metadata
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
- **Viewer**: a connected player who tracks **actor** under `entity-tracking.md`. A player never
  appears in the viewer set of that player.
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
rule rests on captured game artefacts, protocol documentation, or explicit design choices.

### 0.3 ID prefixes

- `USE-PKT-*`: serverbound and clientbound packets and **living flags** bit layout.
- `USE-STR-*`: start preconditions, mode rules, and **use duration** setup.
- `USE-TCK-*`: hold tick progression, speed factor, and natural finish.
- `USE-END-*`: explicit release and hotbar slot cancel.
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
`tools/mcrs_minecraft_probe/traces/item_use.json` (`start_use_main_shield`,
`use_golden_apple_always_eat`, `use_shield_adventure`).

### USE-PKT-002: serverbound stop packets (`41`, `54`)

Two serverbound play packets stop an **active use**:

1. `minecraft:player_action` (`41`) with status `release_use_item` (`6`).
2. `minecraft:set_carried_item` (`54`) with a new hotbar slot (`0` to `8`).

Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`.
Experiment: `USE-E1`; `assets/mcrs/reports/packets.json`;
`tools/mcrs_minecraft_probe/traces/item_use.json` (`release_use_shield`, `switch_slot_cancels_use`,
`release_adv`).

### USE-PKT-003: clientbound `minecraft:set_entity_data` (`101`) index `8` (**living flags**)

The server sends **living flags** at metadata index `8` (`wire_type` `0`, byte, default `0`) in
`minecraft:set_entity_data` (`101`):

| Bit | Mask | Definition |
|-----|------|------------|
| `0` | `0x01` | `1` when **active use** is active, `0` when inactive |
| `1` | `0x02` | `0` for `main` **hand**, `1` for `off` **hand** while bit `0` is `1` |
| `2` | `0x04` | `1` for riptide spin attack, `0` otherwise (`player-pose.md`, `POSE-NET-002`) |

Join metadata omits index `8` while its value equals default `0`.

Provenance: `black_box_confirmed` for default `0`, bit `0` (`main` active = `1`, clear = `0`), and
bit `2` (`0x04`); `documentation` for bit `1` (`0x02`, see `USE-U1`). Kind: `wire`. Confidence:
`high` (bits `0`, `2`), `medium` (bit `1`).
Experiment: `USE-E1`; `crates/mcrs_minecraft_model/tests/fixtures/vanilla/entity_types.json`;
`tools/mcrs_minecraft_probe/traces/item_use.json` (`join_alice`, `join_bob`, `start_use_main_shield`,
`release_use_shield`, `switch_slot_cancels_use`, `use_golden_apple_always_eat`,
`use_shield_adventure`, `release_adv`).

---

## 2. Preconditions and start

### USE-STR-001: game mode and **usable item** gate

`use_item` starts an **active use** when **actor** meets all start conditions.
First, **actor** must be a **loaded client**, must be alive, and must not be in `spectator` mode.
Second, **hand** must hold a **usable item** that passes `USE-STR-002`:

1. Items with `minecraft:blocks_attacks` (such as `minecraft:shield`) start an **active use** in
   both `survival` and `adventure` modes.
2. Empty hands, `spectator` players, and inert items (such as `minecraft:stick`) start no
   **active use**. They emit zero `set_entity_data` packets.
3. Instant-use items (such as `minecraft:snowball`, empty `minecraft:map`, or `minecraft:fishing_rod`)
   emit a swing (`arm-swing.md`, `SWG-SRC-005`) and start no **active use**.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`, `USE-E3`; `assets/mcrs/item_definition/shield.json`, `stick.json`;
`tools/mcrs_minecraft_probe/traces/item_use.json` (`start_use_main_shield`,
`use_shield_adventure`); `tools/mcrs_minecraft_probe/traces/arm_swing.json`
(`use_item_snowball_offhand`, `use_item_empty_map_observers_only`, `use_item_empty_no_swing`,
`use_item_spectator_no_swing`).

### USE-STR-002: consumable gate and **use duration**

When **hand** holds an item with `minecraft:consumable`:

1. When the item has `minecraft:food` with `can_always_eat` equal to `false`, food level must be
   below `20`, or **actor** must be in `creative` mode (`hunger.md`, `HNG-EAT-002`).
2. When food level is `20` and `can_always_eat` is `false` (`minecraft:apple`), the server starts no
   **active use**. It emits zero `set_entity_data`, `sound`, or slot packets.
3. When `can_always_eat` is `true` (`minecraft:golden_apple`), `use_item` starts at food level `20`.
4. For `minecraft:consumable`, **use duration** (`use_duration_ticks`) equals
   `trunc(consume_seconds * 20)` (`32` ticks at default `1.6`).
5. For `minecraft:blocks_attacks`, **active use** stays open until release or cancel.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`; `assets/mcrs/item_definition/apple.json`, `golden_apple.json`, `shield.json`;
`tools/mcrs_minecraft_probe/traces/item_use.json` (`try_apple_at_full_food`,
`use_golden_apple_always_eat`, `start_use_main_shield`).

### USE-STR-003: start state and zero arm swing

At start on `main` **hand**, the server must set bit `0` of **living flags** to `1`.
It must send `set_entity_data` to **actor** and each **viewer**.
It must emit zero `swing_animation` packets (`arm-swing.md`, `SWG-SRC-005`).

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`, `USE-E3`; `tools/mcrs_minecraft_probe/traces/item_use.json`
(`start_use_main_shield`, `start_use_main_then_swap_slot`, `use_golden_apple_always_eat`,
`use_shield_adventure`); `tools/mcrs_minecraft_probe/traces/arm_swing.json`
(`use_item_eat_start_no_swing`, `use_item_bow_no_swing`, `use_item_shield_no_swing`).

---

## 3. Hold ticks, movement speed, and completion

### USE-TCK-001: hold tick progression

While an **active use** continues and **ticks in use** stays below **use duration**, the server
increases **ticks in use** by `1` each server tick.
The server sends zero extra **living flags** packets during hold ticks.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`; `tools/mcrs_minecraft_probe/traces/item_use.json` (`hold_3_ticks`,
`wait_before_slot_cancel`, `wait_adv`).

### USE-TCK-002: movement speed multiplier (`0.2`)

This rule applies when **active use** is active on a non-passenger player and
`minecraft:use_effects` has `can_sprint` equal to `false`.
The movement step must multiply forward and sideways input by `speed_multiplier` (default `0.2`)
and stop sprint starts (`entity-physics.md`, `PHY-INP-001`, `PHY-INP-019`).
When `minecraft:use_effects` specifies another `speed_multiplier` or `can_sprint` equal to `true`
(such as `minecraft:wooden_spear`), the movement step uses those values.

Provenance: `black_box_confirmed`. Kind: `client agreement`. Confidence: `high`.
Experiment: `USE-E2`; `crates/mcrs_minecraft_physics/tests/fixtures/player/e23_sprint_while_using.txt`;
`assets/mcrs/item_definition/bow.json`, `shield.json`, `golden_apple.json`, `wooden_spear.json`.

### USE-TCK-003: consumable natural finish

When **ticks in use** for a `minecraft:consumable` item reaches **use duration**:

1. For food items, **viewers** receive `minecraft:entity.generic.eat` sounds during consumption
   and then `minecraft:entity.player.burp` (volume `0.5`) at completion.
2. The server sends `entity_event` (`status` `9`) to **actor**.
3. The server reduces stack count by `1`. It sends `container_set_slot` to **actor** and
   `set_equipment` to **viewers**.
4. The server applies `on_consume_effects`. For `minecraft:golden_apple`, this adds `regeneration`
   and `absorption`, and sets metadata index `10` effect particles and index `17` float `4.0`.
5. The server clears **active use** and sends **living flags** `0` to **actor** and **viewers**.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`; `assets/mcrs/item_definition/golden_apple.json`;
`tools/mcrs_minecraft_probe/traces/item_use.json` (`use_golden_apple_always_eat`,
`wait_consume_finish`).

---

## 4. Release and cancellation

### USE-END-001: release on `release_use_item`

When **actor** sends `player_action` with status `release_use_item` (`6`) during an **active use**,
the server must clear **active use**.
It must clear bits `0` and `1` of **living flags** to `0` and send `set_entity_data` to **actor**
and each **viewer**.
For non-consumed items such as `minecraft:shield`, the stack stays unchanged and the server emits
zero `sound` or `swing_animation` packets.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`, `USE-E3`; `tools/mcrs_minecraft_probe/traces/item_use.json`
(`release_use_shield`, `release_adv`); `tools/mcrs_minecraft_probe/traces/arm_swing.json`
(`use_item_eat_start_no_swing`).

### USE-END-002: cancel on hotbar slot change

When **actor** sends `set_carried_item` (`54`) to a different hotbar slot during an **active use** in
`main` **hand**, the server must cancel **active use**.
It must clear **living flags** to `0`, send `set_entity_data` to **actor** and each **viewer**, and
send `set_equipment` to each **viewer**.
A cancelled consumable consumes zero items and applies zero effects.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `USE-E1`; `tools/mcrs_minecraft_probe/traces/item_use.json`
(`start_use_main_then_swap_slot`, `wait_before_slot_cancel`, `switch_slot_cancels_use`).

---

## 5. Metadata recipients and edition-neutral contract

### USE-NET-001: self and observer delivery

Every change of **living flags** sends `set_entity_data` index `8` to **actor** and to each
**viewer** in the same dimension (`client-knowledge.md`, `CK-SET-001`; `movement-checks.md`,
`MVC-PKT-031`).
Viewers in `spectator` mode receive the updates. Out-of-range players and players in other
dimensions receive none (`arm-swing.md`, `SWG-DST-001`).

Provenance: `black_box_confirmed`. Kind: `wire`, `semantic`. Confidence: `high`.
Experiment: `USE-E1`; `tools/mcrs_minecraft_probe/traces/item_use.json` (`start_use_main_shield`,
`release_use_shield`, `switch_slot_cancels_use`, `use_golden_apple_always_eat`,
`use_shield_adventure`, `release_adv`); `tools/mcrs_minecraft_probe/traces/arm_swing.json`
(`spectator_observer_receives_punch`, `punch_charlie_out_of_range`, `punch_charlie_in_nether`).

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
  1. Connect `Alice` and `Bob` in `survival` mode (`join_alice`, `join_bob`, `setup_shield`).
  2. Equip `Alice` with `minecraft:shield` in `hotbar.0`, `minecraft:apple` in `off`, and
     `minecraft:stick` in `hotbar.1`.
  3. Send `use_item` (`main`) (`start_use_main_shield`), wait `250` ms (`hold_3_ticks`), and send
     `release_use_item` (`release_use_shield`).
  4. Send `use_item` (`main`) (`start_use_main_then_swap_slot`), wait `150` ms
     (`wait_before_slot_cancel`), and send `select_slot` `1` (`switch_slot_cancels_use`).
  5. Place `minecraft:apple` in `hotbar.0` and `minecraft:golden_apple` in `hotbar.2`. Select slot
     `0` and send `use_item` (`try_apple_at_full_food`).
  6. Select slot `2`, send `use_item` (`use_golden_apple_always_eat`), and wait `1900` ms
     (`wait_consume_finish`).
  7. Switch `Alice` to `adventure` mode (`adventure_mode_setup`) with `minecraft:shield` in
     `hotbar.0`.
  8. Send `use_item` (`use_shield_adventure`), wait `200` ms (`wait_adv`), and send
     `release_use_item` (`release_adv`).
  - Result: `start_use_main_shield`, `start_use_main_then_swap_slot`, and `use_shield_adventure` send
    `index 8 = 1` to `Alice` and `Bob`. `Bob` receives `set_equipment` before `set_entity_data`.
  - Result: wait steps emit zero metadata. `release_use_shield`, `release_adv`, and
    `switch_slot_cancels_use` send `index 8 = 0` to `Alice` and `Bob`.
  - Result: `try_apple_at_full_food` emits zero packets. `use_golden_apple_always_eat` sends
    `index 8 = 1`, `8` eat sounds, `1` burp sound, `entity_event`, slot `38` set to `null`,
    `index 8 = 0`, `index 10` particles, and `index 17` value `4.0`.
- **USE-E2** (`crates/mcrs_minecraft_physics/tests/fixtures/player/e23_sprint_while_using.txt`):
  1. Hold `use_item` with a bow, hold forward and sprint for two seconds, then send
     `release_use_item`.
  - Result: horizontal speed stays scaled by `0.2` without sprint until `release_use_item`.
- **USE-E3** (`tools/mcrs_minecraft_probe/traces/arm_swing.json`):
  1. Send `use_item` for `apple` (`use_item_eat_start_no_swing`), `bow`, `shield`, `snowball`
     (`off`), empty hand, and `spectator`.
  - Result: continuous start and release emit zero `swing_animation` packets. Empty hand and
    `spectator` start nothing; instant uses swing without **active use**.

---

## 9. Unresolved

- **USE-U1** (off-hand **living flags** bit `1` and same-slot item replacement):
  `tools/mcrs_minecraft_probe/traces/item_use.json` only tests `hand: "main"`. Experiment: add an
  `off`-hand `shield` hold and release plus a mid-eat slot overwrite to `item_use.json` to confirm
  `index 8 = 3` and stack-change cancel behaviour.
- **USE-U2** (`blocks_attacks` delay and damage reduction): `block_delay_seconds` (`0.25` s) belongs
  to `entity-damage.md` (`DMG-HIT-005`).
- **USE-U3** (`use_item_on` consumable fallback): block-place refusal fallback to `consumable`
  (`block-placing.md`, `PLC-INV-009`) needs a **living flags** probe.
