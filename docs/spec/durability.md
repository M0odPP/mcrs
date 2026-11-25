# Item durability: behavioural specification

Version: see `assets/minecraft/version.json`. Wire packet IDs: `assets/mcrs/reports/packets.json`.
Item definitions: `assets/mcrs/item_definition/`.

This specification defines player item **wear**, `minecraft:unbreaking` reduction, **break events**,
and `mcrs:durability_extra_use`. Stack component shapes and base patch rules live in
[item-stack.md](item-stack.md).

## 0. Conventions

### 0.1 Terms

- **Durability**: available wear capacity `max_damage - damage` on a damageable stack
  (`item-stack.md`, `STK-DMG-001`..`STK-DMG-004`).
- **Wear**: non-negative integer increment added to `minecraft:damage` on a stack (`STK-DMG-005`).
- **Hit**: a damage event received by a target (`entity-damage.md`, `DMG-HIT-003`) or an accepted
  melee attack (`player-attack.md`, `ATK-DMG-004`).
- **Armour slot**: one of the four equipped armour slots `P[36..39]` (`inventory.md`,
  `INV-SLT-001`): `feet` (`36`), `legs` (`37`), `chest` (`38`), and `head` (`39`).
- **Break event**: clientbound `minecraft:entity_event` (`34`) with status byte `47`, `48`, or
  `49`..`52` emitted when wear breaks a stack (`STK-DMG-006`).

### 0.2 Provenance and kinds

Rule tags follow `registry.md`, section 0.3, and `docs/spec/README.md` (`black_box_confirmed`,
`documentation`, or `design`; kind `wire`, `client agreement`, or `semantic`). Rules rely on
experiments against a vanilla server (`assets/minecraft/version.json`) and on
`tools/mcrs_minecraft_probe/traces/durability.json`.

### 0.3 Artefacts cited

- `tools/mcrs_minecraft_probe/scenarios/durability.json`
- `tools/mcrs_minecraft_probe/traces/durability.json`
- `assets/mcrs/item_definition/` (`golden_pickaxe`, `iron_sword`, `wooden_sword`, `golden_helmet`,
  `golden_chestplate`, `golden_leggings`, `golden_boots`)
- `assets/mcrs/reports/packets.json`
- `docs/spec/item-stack.md` (`STK-DMG-001`..`STK-DMG-008`, `STK-SHP-020`, `STK-SHP-028`)
- `docs/spec/block-breaking.md` (`BRK-DMG-001`..`BRK-DMG-003`)
- `docs/spec/player-attack.md` (`ATK-DMG-004`, `ATK-DMG-005`)
- `docs/spec/entity-damage.md` (`DMG-HIT-003`, `DMG-RED-001`)
- `docs/spec/inventory.md` (`INV-SLT-001`, `INV-SLT-003`)
- `docs/spec/player-death.md` (`PLD-EQP-001`, `PLD-EQP-002`, `PLD-EQP-004`)
- `docs/spec/client-entities.md` (`CENTS-EQUIP-001`)
- `docs/spec/registry-fields.md` (`REG-FLD-537`)

---

## 1. Eligibility and wear application

### DUR-ELG-001: damageable stack and `minecraft:unbreakable`

A stack takes **wear** only when `STK-DMG-001` holds: effective `minecraft:max_damage`, effective
`minecraft:damage`, and no effective `minecraft:unbreakable`. A `wooden_sword` with
`minecraft:unbreakable` takes `0` **wear** when it kills a pig.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `DUR-E6`; `tools/mcrs_minecraft_probe/traces/durability.json`
(`attack_unbreakable`, `inspect_unbreakable`).

### DUR-ELG-002: Creative mode skip

Players in Creative mode must take `0` **wear** on block breaks, melee hits, and damage hits.

Provenance: `documentation`. Kind: `semantic`. Confidence: `medium`.

### DUR-ELG-003: apply wear and break at `max_damage`

After enchantment rolls (`DUR-ENC-001`, `DUR-ENC-002`), the holder applies integer **wear** `a` via
`STK-DMG-005`. When `minecraft:damage` reaches `max_damage`, the stack breaks (`STK-DMG-006`) and
emits a **break event** (`DUR-NET-001`).

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `DUR-E1`, `DUR-E5`; `tools/mcrs_minecraft_probe/traces/durability.json`
(`stop_break_1`, `stop_break_2`, `test_armour_wear_and_slot_events`).

---

## 2. Wear on block break

### DUR-BRK-001: main-hand tool `damage_per_block`

When a non-Creative player breaks a block with hardness above `0`, the main-hand stack takes
`damage_per_block` **wear** from effective `minecraft:tool` (`BRK-DMG-001`, `STK-SHP-020`). Stacks
without `minecraft:tool` take `0` **wear** (`BRK-DMG-001`).

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `DUR-E1`; `tools/mcrs_minecraft_probe/traces/durability.json`
(`stop_break_1`, `stop_break_2`).

### DUR-BRK-002: tool `rules` and default `damage_per_block`

Entries in `rules` on `minecraft:tool` match specific blocks (`STK-SHP-020`), while
`damage_per_block` sets the **wear** cost per broken block. Tools default `damage_per_block` to `1`;
weapons such as `iron_sword` set `2`.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `DUR-E1` (`golden_pickaxe`), `assets/mcrs/item_definition/` (`iron_sword.json`).

### DUR-BRK-003: zero-hardness blocks

When a player breaks a block with hardness `0`, such as `slime_block`, the main-hand tool takes
`0` **wear** (`BRK-DMG-001`).

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `DUR-E2`; `tools/mcrs_minecraft_probe/traces/durability.json`
(`break_slime`, `inspect_diamond_pick`).

### DUR-BRK-004: Creative `can_destroy_blocks_in_creative`

Creative block breaks must apply `0` **wear** even when `can_destroy_blocks_in_creative` allows
the break.

Provenance: `documentation`. Kind: `semantic`. Confidence: `medium`.

---

## 3. Wear on melee hit

### DUR-ATK-001: main-hand `minecraft:weapon` cost

When a non-Creative player lands an accepted melee **hit** (`ATK-DMG-004`), the main-hand stack
takes `item_damage_per_attack` **wear** from effective `minecraft:weapon` (`REG-FLD-537`, default
`1`). Stacks without `minecraft:weapon` take `0` **wear**.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `DUR-E4`; `tools/mcrs_minecraft_probe/traces/durability.json`
(`attack_pig_sword`, `inspect_sword_damage`).

### DUR-ATK-002: rejected melee hits

Rejected melee attacks (invalid target, invulnerable target, cooldown block, or Creative
attacker) must apply `0` weapon **wear** (`ATK-DMG-004`, `ATK-DMG-005`).

Provenance: `documentation`. Kind: `semantic`. Confidence: `medium`.

---

## 4. Armour wear on damage hit

### DUR-ARM-001: `#minecraft:bypasses_armor` gate

Armour **wear** triggers when a non-Creative target takes a positive damage **hit** whose damage
type lacks tag `#minecraft:bypasses_armor` (`DMG-RED-001`, `DMG-HIT-003`).

Provenance: `documentation`. Kind: `semantic`. Confidence: `medium`.

### DUR-ARM-002: pre-reduction `raw_amount` formula

Before enchantments, each eligible armour stack takes `max(1, floor(raw_amount / 4))` **wear**,
where `raw_amount` is the pre-reduction **hit** amount. Hit amount `4` (`minecraft:mob_attack`)
inflicts `1` **wear** per piece.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `DUR-E5`; `tools/mcrs_minecraft_probe/traces/durability.json`
(`test_armour_wear_and_slot_events`).

### DUR-ARM-003: slot placement and `damage_on_hurt`

Damage **hit** **wear** applies only to **armour slots** (`P[36..39]`, `INV-SLT-001`, `INV-SLT-003`)
whose effective `minecraft:equippable` has `damage_on_hurt = true` (`STK-SHP-028`) and pass
`DUR-ELG-001`.

Provenance: `documentation`. Kind: `semantic`. Confidence: `medium`.

### DUR-ARM-004: independent per-slot break evaluation

All four **armour slots** evaluate **wear** and break independently on the same **hit**. One **hit**
can break head, chest, legs, and feet stacks together.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `DUR-E5`; `tools/mcrs_minecraft_probe/traces/durability.json`
(`test_armour_wear_and_slot_events`).

---

## 5. `minecraft:unbreaking` enchantment

### DUR-ENC-001: non-armour retention chance

When a non-armour stack holds `minecraft:unbreaking` at level `L > 0`, each unit of base **wear**
rolls independently. A unit stays active with probability `1 / (L + 1)` and cancels otherwise
(`BRK-DMG-003`).

Provenance: `documentation`. Kind: `semantic`. Confidence: `high`.

### DUR-ENC-002: armour retention chance

When an armour stack holds `minecraft:unbreaking` at level `L > 0`, each unit of base **wear**
stays active with probability `0.6 + 0.4 / (L + 1)` and cancels otherwise.

Provenance: `documentation`. Kind: `semantic`. Confidence: `medium`.

### DUR-ENC-003: stochastic skip observation

Across four `stone` breaks with `golden_pickaxe` enchanted with `minecraft:unbreaking` level `3`,
all four **wear** rolls can cancel and leave `0` `minecraft:damage`.

Provenance: `black_box_confirmed`. Kind: `semantic`. Confidence: `high`.
Experiment: `DUR-E3`; `tools/mcrs_minecraft_probe/traces/durability.json`
(`setup_unbreaking_3_pick`..`inspect_unbreaking_3`).

---

## 6. Wire effects and break events

### DUR-NET-001: active slot break events (`47`, `49`..`52`)

When **wear** breaks a stack, the server sends `minecraft:entity_event` (`34`) to the holder and
nearby observers with this status byte:

- `47`: main hand (`BRK-DMG-002`, `stop_break_2`);
- `49`: feet (`test_armour_wear_and_slot_events`);
- `50`: legs (`test_armour_wear_and_slot_events`);
- `51`: chest (`test_armour_wear_and_slot_events`);
- `52`: head (`test_armour_wear_and_slot_events`).

Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`.
Experiment: `DUR-E1`, `DUR-E5`; `tools/mcrs_minecraft_probe/traces/durability.json`.

### DUR-NET-002: break event order before world and damage packets

On a block-break **break event**, `minecraft:entity_event` (`34`) arrives before
`minecraft:block_update` (`stop_break_2`). On an armour-break **hit**, `minecraft:entity_event`
(`34`) arrives before `minecraft:damage_event` (`25`), `minecraft:set_entity_data` (`101`), and
`minecraft:set_health` (`106`).

Provenance: `black_box_confirmed`. Kind: `client agreement`. Confidence: `high`.
Experiment: `DUR-E1`, `DUR-E5`; `tools/mcrs_minecraft_probe/traces/durability.json`
(`stop_break_2`, `test_armour_wear_and_slot_events`).

### DUR-NET-003: holder slot sync and main-hand equipment sync

Main-hand **wear** without break sends `minecraft:container_set_slot` (`20`, slot `36`) to the
holder and `minecraft:set_equipment` (`104`) to observers (`stop_break_1`, `attack_pig_sword`).
Main-hand break clears slot `36` (`stack: null`) and sends `set_equipment` (`stop_break_2`);
armour break clears holder slots `5..8` (`stack: null`, `test_armour_wear_and_slot_events`).

Provenance: `black_box_confirmed`. Kind: `wire`. Confidence: `high`.
Experiment: `DUR-E1`, `DUR-E4`, `DUR-E5`; `tools/mcrs_minecraft_probe/traces/durability.json`.

### DUR-NET-004: offhand break event (`48`)

When **wear** breaks a stack in the offhand slot (`P[40]`, `INV-SLT-001`), the **break event**
status byte on `minecraft:entity_event` (`34`) is `48`.

Provenance: `documentation`. Kind: `wire`. Confidence: `medium`.

### DUR-NET-005: full equipment sync across non-break armour wear

General observer `minecraft:set_equipment` (`104`) updates on armour slots (`slot_id` `2..5`) and
component changes follow `player-death.md` (`PLD-EQP-001`, `PLD-EQP-002`, `PLD-EQP-004`) and
`client-entities.md` (`CENTS-EQUIP-001`).

Provenance: `documentation`. Kind: `wire`. Confidence: `medium`.

---

## 7. Extension hook

### DUR-EDN-001: `mcrs:durability_extra_use`

Gameplay features outside block break, melee **hit**, and damage **hit** must request **wear** via
`mcrs:durability_extra_use`. Each event gives target entity, slot, and base **wear** `a >= 1`,
and runs `DUR-ELG-001`..`DUR-ELG-003`, `DUR-ENC-001`..`DUR-ENC-002`, and
`DUR-NET-001`..`DUR-NET-005`.

Provenance: `design`. Kind: `semantic`. Confidence: `high`.

---

## 8. Out of scope

- Shield block **wear** (`minecraft:blocks_attacks`) and special tool uses such as shears or flint.
- `minecraft:mending` experience repair, Grindstone repair, Anvil repair, and offhand **wear**
  triggers (`DUR-NET-004` defines only the status byte `48`).
- Non-player mob armour **wear**.
- Server sound packets on stack break (`sounds: []` in `stop_break_2` and
  `test_armour_wear_and_slot_events`; clients play break audio and particles from `entity_event`).

---

## 9. Experiments

All experiments below run in `tools/mcrs_minecraft_probe/scenarios/durability.json` with trace
`tools/mcrs_minecraft_probe/traces/durability.json` (Alice and observer Bob).

### DUR-E1: tool wear and break on `stone`

1. Give Alice `golden_pickaxe[damage=30]` in slot `0`.
2. Break two `stone` blocks (`stop_break_1`, `stop_break_2`).
3. Verify first break sends `container_set_slot` (`36`) and Bob's `set_equipment` with `0` entity
   events.
4. Verify second break sends `entity_event` `47` before `block_update` to both clients and clears
   slot `36` (`null`).

### DUR-E2: zero-hardness block

1. Give Alice `diamond_pickaxe` and break `slime_block` (`break_slime`, `inspect_diamond_pick`).
2. Verify `0` slot updates, `0` entity events, and no `minecraft:damage` on `diamond_pickaxe`.

### DUR-E3: `minecraft:unbreaking` level `3`

1. Give Alice `golden_pickaxe[enchantments={"minecraft:unbreaking":3}]`.
2. Break four `stone` blocks (`setup_unbreaking_3_pick`..`inspect_unbreaking_3`).
3. Verify stochastic skip can leave `0` `minecraft:damage` after all four breaks.

### DUR-E4: weapon melee hit

1. Give Alice `iron_sword` and strike a pig once (`attack_pig_sword`, `inspect_sword_damage`).
2. Verify `container_set_slot` (`36`) updates, Bob gets `set_equipment`, and `iron_sword` holds
   `minecraft:damage: 1`.

### DUR-E5: armour wear and slot break events

1. Equip Alice via `/item replace` with `golden_helmet[damage=76]`, `golden_chestplate[damage=111]`,
   `golden_leggings[damage=104]`, and `golden_boots[damage=90]`.
2. Apply `/damage Alice 4 minecraft:mob_attack` (`test_armour_wear_and_slot_events`,
   `inspect_armour_after`).
3. Verify Alice survives, `container_set_slot` clears slots `5..8` (`null`), and both clients get
   `entity_event` `52`, `51`, `50`, `49` before `damage_event` (`25`), `set_entity_data` (`101`),
   and `set_health` (`106`).
4. Verify `inspect_armour_after` reports empty entity equipment (`{}`).

### DUR-E6: `minecraft:unbreakable` weapon

1. Give Alice `wooden_sword[minecraft:unbreakable={}]` and kill a pig (`attack_unbreakable`,
   `inspect_unbreakable`).
2. Verify `0` wear updates and `0` `minecraft:damage` on `wooden_sword`.
