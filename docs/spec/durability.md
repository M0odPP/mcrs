# Item durability — clean-room specification

Protocol version: `1073742169` (`26.4-snapshot-7`).

This specification defines Wave-1 item wear rules for players across block break, melee hit,
hurt hit, `minecraft:unbreaking`, break events, and `mcrs:durability_extra_use`. Stack patch
rules live in [item-stack.md](item-stack.md).

### 0.1 Provenance

Every rule carries a provenance tag from `docs/spec/registry.md`:

- `black_box_confirmed`: observed in `tools/mcrs_minecraft_probe/traces/durability.json` or the
  dumped definition and packet reports.
- `documentation`: drawn from public documentation and prior documented mechanics.
- `design`: project extension rule for issue `#118`.

### 0.2 Artefacts cited

- `tools/mcrs_minecraft_probe/traces/durability.json`: vanilla probe trace recorded on
  `26.4-snapshot-7` (protocol `1073742169`).
- `assets/mcrs/item_definition/`: default component definitions per item (`golden_pickaxe`,
  `iron_sword`, `wooden_sword`, `golden_helmet`, `golden_chestplate`, `golden_leggings`,
  `golden_boots`).
- `assets/mcrs/reports/packets.json`: packet names and numeric identifiers for `26.4-snapshot-7`.
- `docs/spec/item-stack.md`: `STK-DMG-001`..`STK-DMG-008`, `STK-SHP-006`..`STK-SHP-009`,
  `STK-SHP-015`, `STK-SHP-020`, `STK-SHP-028`.
- `docs/spec/block-breaking.md` (`BRK-DMG-001`..`BRK-DMG-003`), `docs/spec/player-attack.md`
  (`ATK-DMG-004`, `ATK-DMG-005`), `docs/spec/entity-damage.md` (`DMG-RED-001`),
  `docs/spec/inventory.md` (`INV-SLT-001`..`INV-SLT-002`, `INV-MNU-001`),
  `docs/spec/registry-fields.md` (`REG-FLD-537`).

---

## 1. Eligibility and wear application

**DUR-ELG-001** (`black_box_confirmed`, semantic). A stack takes wear only when `STK-DMG-001` holds:
it has effective `minecraft:max_damage`, effective `minecraft:damage`, and no effective
`minecraft:unbreakable`. In `durability.json`, a `wooden_sword` with `minecraft:unbreakable` takes
`0` wear when it kills a pig (`attack_unbreakable`, `inspect_unbreakable`).

**DUR-ELG-002** (`documentation`, semantic). Players in Creative mode skip all item wear on block
break, melee hit, and hurt hit.

**DUR-ELG-003** (`black_box_confirmed`, semantic). After enchantment checks (`DUR-ENC-001`,
`DUR-ENC-002`), the holder applies the integer wear `a` via `STK-DMG-005`. When `minecraft:damage`
reaches `max_damage`, the stack breaks (`STK-DMG-006`) and triggers `DUR-NET-001`.

---

## 2. Wear on block break

**DUR-BRK-001** (`black_box_confirmed`, semantic). When a non-Creative player breaks a block with
hardness above `0`, the main-hand stack takes `damage_per_block` wear (`BRK-DMG-001`,
`STK-SHP-020`). That count comes from effective `minecraft:tool`; stacks without `minecraft:tool`
take `0` wear (`BRK-DMG-002`).

In `durability.json`, `golden_pickaxe` (`max_damage` `32`, `damage_per_block` `1`) breaks `stone`
at `damage` `30` to reach `31` (`stop_break_1`), then breaks at `31` (`stop_break_2`).

**DUR-BRK-002** (`black_box_confirmed`, semantic). Entries in `rules` on `minecraft:tool` select
block-specific properties (`STK-SHP-020`), while `damage_per_block` sets the wear per broken
block. Tools default `damage_per_block` to `1`; weapons such as `iron_sword` set it to `2`.

**DUR-BRK-003** (`black_box_confirmed`, semantic). When a player breaks a block with hardness `0`,
such as `slime_block`, the main-hand tool takes `0` wear (`BRK-DMG-003`). Confirmed in
`durability.json`: `break_slime` and `inspect_diamond_pick` leave `diamond_pickaxe` undamaged.

**DUR-BRK-004** (`documentation`, semantic). Creative block breaks must apply `0` wear even when
`can_destroy_blocks_in_creative` lets the player break the block.

---

## 3. Wear on melee hit

**DUR-ATK-001** (`black_box_confirmed`, semantic). When a non-Creative player lands an accepted
melee hit (`ATK-DMG-004`), the main-hand stack takes `item_damage_per_attack` wear (`REG-FLD-537`,
default `1`). Wear uses effective `minecraft:weapon`; stacks without `minecraft:weapon` take `0`
wear.

Confirmed in `durability.json`: `iron_sword` (`weapon: {}`, `max_damage` `250`) strikes a pig once
(`attack_pig_sword`) and gets `minecraft:damage: 1` (`inspect_sword_damage`).

**DUR-ATK-002** (`documentation`, semantic). Rejected melee hits (invalid target, invulnerable
target, cooldown gate, or Creative attacker) must apply `0` weapon wear (`ATK-DMG-004`,
`ATK-DMG-005`).

---

## 4. Armour wear on hurt hit

**DUR-ARM-001** (`documentation`, semantic). Armour hit wear triggers when a non-Creative target
takes a positive hit whose damage type lacks tag `#minecraft:bypasses_armor` (`DMG-RED-001`).

**DUR-ARM-002** (`black_box_confirmed`, semantic). Before enchantments, each eligible armour piece
takes `max(1, floor(raw_amount / 4))` wear, where `raw_amount` is the pre-reduction hit amount. In
`durability.json`, hit amount `4` (`minecraft:mob_attack`) inflicts `1` wear per piece
(`test_armour_wear_and_slot_events`).

**DUR-ARM-003** (`documentation`, semantic). Hit wear applies only to armour slots (`P[36..39]`,
`INV-SLT-002`) with `damage_on_hurt = true` (`STK-SHP-028`) when `DUR-ELG-001` holds.

**DUR-ARM-004** (`black_box_confirmed`, semantic). All four armour slots evaluate wear and break
independently on the same hit. In `durability.json` (`test_armour_wear_and_slot_events`), one hit
breaks `golden_helmet`, `golden_chestplate`, `golden_leggings`, and `golden_boots` together.

---

## 5. `minecraft:unbreaking` enchantment

**DUR-ENC-001** (`documentation`, semantic). When a non-armour stack holds `minecraft:unbreaking`
at level `L > 0`, each unit of base wear rolls independently. A unit stays active with probability
`1 / (L + 1)` and cancels otherwise.

**DUR-ENC-002** (`documentation`, semantic). When an armour stack holds `minecraft:unbreaking` at
level `L > 0`, each unit of base wear stays active with probability `0.6 + 0.4 / (L + 1)` and
cancels otherwise.

**DUR-ENC-003** (`black_box_confirmed`, semantic). Across four `stone` breaks with
`golden_pickaxe` enchanted with `minecraft:unbreaking` level `3`, all four wear rolls can cancel.
Confirmed in `durability.json` (`setup_unbreaking_3_pick`..`inspect_unbreaking_3`), which ends
with `0` `minecraft:damage`.

---

## 6. Wire effects and break events

**DUR-NET-001** (`black_box_confirmed`, wire). When wear breaks a stack in an active slot, the
server sends `minecraft:entity_event` (packet `34`) for that entity with this status byte:

- `47`: main hand (`DUR-BRK-001`, `stop_break_2`);
- `48`: offhand (`documentation`);
- `49`: feet (`test_armour_wear_and_slot_events`);
- `50`: legs (`test_armour_wear_and_slot_events`);
- `51`: chest (`test_armour_wear_and_slot_events`);
- `52`: head (`test_armour_wear_and_slot_events`).

Both the holder client and nearby observers receive `minecraft:entity_event`.

**DUR-NET-002** (`black_box_confirmed`, client agreement). On a break event, `minecraft:entity_event`
(`34`) arrives before `minecraft:block_update` (`stop_break_2`). On a lethal armour hit, it arrives
before `minecraft:damage_event` (`25`), `minecraft:set_entity_data` (`101`), and
`minecraft:set_health` (`106`) (`test_armour_wear_and_slot_events`).

**DUR-NET-003** (`black_box_confirmed`, wire). A stack change without break sends
`minecraft:container_set_slot` (`20`) to the holder (`36` for main hand, `5..8` for head..feet).
Observers receive `minecraft:set_equipment` (`104`) (`stop_break_1`, `attack_pig_sword`). On
break, `container_set_slot` carries `null` (`stack: null`) and `set_equipment` clears the slot.

---

## 7. Extension hook

**DUR-EDN-001** (`design`, semantic). Gameplay features outside block break, melee hit, and hurt
hit must trigger wear via `mcrs:durability_extra_use`. Each event carries target entity, slot, and
base wear `a >= 1`. It follows `DUR-ELG-001`..`DUR-ELG-003`, `DUR-ENC-001`..`DUR-ENC-002`, and
`DUR-NET-001`..`DUR-NET-003`.

---

## 8. Out of scope

- Shield block wear (`minecraft:blocks_attacks`) and special tool uses such as shears or flint.
- `minecraft:mending` experience repair, Grindstone repair, Anvil repair, and offhand triggers.
- Non-player mob armour wear.
- Server sound packets on break (`sounds: []` in `stop_break_2` and
  `test_armour_wear_and_slot_events`; clients play sound and particles from `entity_event`).

---

## 9. Experiments

All experiments below come from `tools/mcrs_minecraft_probe/scenarios/durability.json` and run in
`tools/mcrs_minecraft_probe/traces/durability.json` (`26.4-snapshot-7`, Alice and observer Bob).

**DUR-E1 (Tool wear and break on `stone`).** Give Alice `golden_pickaxe[damage=30]` in slot `0`
and break two `stone` blocks (`stop_break_1`, `stop_break_2`). Expected: first break updates slot
`36` and equipment with `0` entity events. Second break sends `entity_event` `47` to Alice and Bob
before `block_update`, and clears slot `36` (`null`).

**DUR-E2 (Zero-hardness block).** Break `slime_block` with `diamond_pickaxe` (`break_slime`,
`inspect_diamond_pick`). Expected: `0` slot updates, `0` entity events, and no `minecraft:damage`
on `diamond_pickaxe`.

**DUR-E3 (`minecraft:unbreaking` level `3`).** Break four `stone` blocks with
`golden_pickaxe[enchantments={"minecraft:unbreaking":3}]` (`setup_unbreaking_3_pick`..
`inspect_unbreaking_3`). Expected: stochastic skip allows `0` `minecraft:damage` after all four
breaks.

**DUR-E4 (Weapon melee hit).** Strike a pig once with `iron_sword` (`attack_pig_sword`,
`inspect_sword_damage`). Expected: `container_set_slot` (`36`) updates and `iron_sword` holds
`minecraft:damage: 1`.

**DUR-E5 (Armour wear and slot breaks).** Equip Alice with `golden_helmet[damage=76]`,
`golden_chestplate[damage=111]`, `golden_leggings[damage=104]`, and `golden_boots[damage=90]`.
Apply `/damage Alice 4 minecraft:mob_attack` (`test_armour_wear_and_slot_events`,
`inspect_armour_after`). Expected: slots `5..8` turn `null`, both clients receive `entity_event`
`52`, `51`, `50`, `49` before `damage_event` (`25`), and equipment clears.

**DUR-E6 (`minecraft:unbreakable` weapon).** Kill a pig with
`wooden_sword[minecraft:unbreakable={}]` (`attack_unbreakable`, `inspect_unbreakable`). Expected:
`0` wear updates and `0` `minecraft:damage` on `wooden_sword`.
