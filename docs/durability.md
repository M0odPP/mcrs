# Item Durability Design Note (`#118`)

Wave-1 player item durability spans Layers 3–5 and implements
[docs/spec/durability.md](spec/durability.md) plus `STK-DMG-001`..`STK-DMG-008` in
[docs/spec/item-stack.md](spec/item-stack.md).

## State Classification

| State | Kind | Owner / Lifetime |
| :--- | :--- | :--- |
| `ItemStack` (`kind`, `count`, `patch`) | Authoritative truth | `mcrs_minecraft_item_stack` (`Damage`, `MaxDamage`, `Unbreakable`, `Tool`, `Equippable`, `Weapon`, `Enchantments`) |
| Stack entity `Effective<T>`, `Stacks`, `InSlot`, `Slots`, `Equipped` | Materialized derived | `mcrs_minecraft_engine_item`; `Equipped` marks only the holder's `Selected` stack |
| `PlayerGameMode`, `Blocks` hardness, `DamageTypeRules` | Authoritative truth | `mcrs_minecraft_game_mode`, `mcrs_minecraft_block`, `mcrs_minecraft_damage` |
| `BlockBroken`, melee hit trigger, `Damaged`, `DurabilityExtraUse` | Temporal tick fact | Produced in `ClientInput`, `PacketReport`, `WorldWrite`/`DamageSet::Apply`, consumed same tick |
| `MenuSync` diffs -> `ClientboundContainerSetSlot`, `PlayerSignal` / `TileEvents` | Temporal output fact | Synced in `InventoryServerSet::Sync` / `FixedPostUpdate` -> `take_output` |

No long-lived wear accumulator component exists on players or stacks; wear resolves in the same
tick that emits the triggering message.

## Layer and Crate Map

```text
Layer 3  mcrs_minecraft_item_stack          -- STK-DMG-001..008 stack helpers + Weapon promotion (alloc + std::io)
Layer 3  mcrs_minecraft_durability          -- #![no_std] scalar kernel: wear formulas, Unbreaking RNG, SlotBreakEvent
Layer 4  mcrs_minecraft_engine_damage       -- pre-reduction raw_amount on HitReport/Damaged + melee wear trigger msg
Layer 5  mcrs_minecraft_vanilla_durability  -- sparse message-driven plugin + mcrs:durability_extra_use
Layer 7  mcrs_minecraft_server              -- registers VanillaDurabilityPlugin per dimension
```

This keeps the layer DAG in `AGENTS.md` strictly acyclic: `#![no_std]` `mcrs_minecraft_durability`
depends only on `mcrs_minecraft_entity_key` (`KeyedStream::unit(&self, draw: u64)`), never on
`mcrs_minecraft_item_stack`, and Layer-5 `mcrs_minecraft_vanilla_durability` depends on Layer-4
messages rather than Layer-5 `mcrs_minecraft_vanilla_player_attack`.

## 1. Stack-Level API (`mcrs_minecraft_item_stack`)

`mcrs_minecraft_item_stack` (Layer 3, `extern crate alloc`, `std::io`) already models `MaxDamage`,
`Damage`, `Unbreakable`, `Tool`, `Equippable`, and `Enchantments` via `for_each_builtin!`.

Proposed `STK-DMG-001`..`STK-DMG-008` helpers on `mcrs_minecraft_item_stack::ItemStack` take an
effective-defaults lookup trait implemented in `mcrs_minecraft_item_table` for `Prototype` (keeping
`mcrs_minecraft_item_table -> mcrs_minecraft_item_stack` acyclic) and a proposed `BreakBehavior`
enum `{ BreakAtMax, ClampBelowMax }` instead of a bare `bool`:

- `is_damageable`, `damage_value`, `set_damage`, `is_damaged`, `is_broken`,
  `will_break_on_next_damage` (`STK-DMG-001`..`STK-DMG-004`, `STK-DMG-008`).
- `apply_damage`: executes `STK-DMG-005`..`STK-DMG-007`, returning a break outcome when
  `BreakAtMax` reaches `max_damage` and clamping to `max_damage - 1` under `ClampBelowMax`.

### ECS `Damage` Write Rule

Inside `mcrs_minecraft_engine_item`, `Effective<Damage>` defaults to `0` when `Damage` is absent
from the stack entity's archetype (`Query<&mut Damage>` misses fresh items at `damage = 0`).

Write stack wear in Layer 5 with this exact rule:

1. If `Damage` exists on the stack entity (`Query<&mut Damage>` matches), mutate it in place and
   call `commands.entity(stack_entity).touch()`.
2. Otherwise, call `commands.entity(stack_entity).set(Damage(new_damage))` (`StackCommandsExt::set`).
3. If `new_damage >= max_damage`, call `commands.entity(stack_entity).consume(1)`
   (`StackCommandsExt::consume`) and emit the slot break event.

## 2. `#![no_std]` Kernel (`mcrs_minecraft_durability`)

Following `mcrs_minecraft_block_break` and `mcrs_minecraft_swing`, `#![no_std]`
`mcrs_minecraft_durability` operates purely on scalar inputs and typed enums (no bare `bool`
parameters, no `ItemStack` dependency):

- `WearKind` `{ Tool, Armour }`: selects retention probability (`1.0 / (level + 1) as f64` for
  `Tool`, `0.6 + 0.4 / (level + 1) as f64` for `Armour`).
- `DurabilityFlag` `{ Damageable, Unbreakable }`, `ArmourBypass` `{ Normal, BypassesArmour }`,
  `SlotBreakEvent` `{ MainHand = 47, OffHand = 48, Feet = 49, Legs = 50, Chest = 51, Head = 52 }`.
- `roll_unbreaking(amount: i32, unbreaking_level: i32, kind: WearKind, stream: &KeyedStream) -> i32`:
  returns `amount` unchanged when `unbreaking_level <= 0`; else samples `stream.unit(draw)` (`[0, 1)`)
  for each unit of `amount`.
- `block_break_wear(damage_per_block: i32, block_hardness: f32) -> i32`: returns `0` when
  `block_hardness <= 0.0`, else `damage_per_block.max(0)`.
- `attack_wear(item_damage_per_attack: i32) -> i32`: returns `item_damage_per_attack.max(0)`.
- `armour_hit_wear(raw_amount: f32, hurt_policy: HurtWearPolicy, bypass: ArmourBypass) -> i32`:
  returns `0` on `BypassesArmour`, disabled `hurt_policy`, or `raw_amount <= 0.0`; else
  `(raw_amount / 4.0).floor().max(1.0) as i32`.

## 3. Sparse Layer-5 Plugin & Tick Schedule

`VanillaDurabilityPlugin` uses zero per-player or per-item `tick` iteration, scheduling four
sparse `MessageReader` systems in `FixedUpdate`:

1. **`on_block_broken` (`TickSet::Simulate`)**: reads `BlockBroken` (written in
   `TickSet::ClientInput` by `break_blocks` / `advance_digs`), skips `GameMode::Creative`, checks
   `Blocks::state(msg.state).hardness` and the holder's `Selected` stack (`Effective<Tool>`),
   applies `block_break_wear`, and emits `SlotBreakEvent::MainHand` (`47`) on break.
2. **`on_melee_attack` (`TickSet::Simulate`)**: reads the Layer-4 accepted melee-hit trigger
   emitted by `mcrs_minecraft_vanilla_player_attack` (`DamageSet::PacketReport`), skips
   `GameMode::Creative`, inspects the attacker's `Selected` stack (`Effective<Weapon>`), and
   applies `attack_wear`.
3. **`on_entity_damaged` (`TickSet::Project.after(DamageSet::Report)`)**: `DamagePlugin` runs
   `DamageSet::Apply..Report` in `TickSet::WorldWrite.after(MotionPass)`, so armour wear MUST run in
   `TickSet::Project.after(DamageSet::Report)` alongside `PlayerVitalsPlugin::signal_player_hits`.
   Skips rejected hits and `GameMode::Creative`, checks `DamageTypeRules::tags` for
   `BYPASSES_ARMOR`, iterates **`Slots`** indices `ArmourSlot::{Feet, Legs, Chest, Head}` (`36..=39`,
   since `Equipped` tracks only `Selected`), checks `Effective<Equippable>` (`damage_on_hurt`),
   and emits `SlotBreakEvent` (`49..=52`) on break.
4. **`on_extra_use` (`TickSet::Project.after(DamageSet::Report)`)**: consumes `DurabilityExtraUse`
   (`mcrs:durability_extra_use`) from both `Simulate` and `WorldWrite` callers in the same tick.

### Wire Sync & Break Event Ordering Seam (`DUR-NET-002`)

`InventoryServerSet::Sync` (`sync_menus`) runs `.after(TickSet::Project).before(TickSet::Output)`
and diffs `MenuSync` against `Slots`/`Stacks` to emit `ClientboundContainerSetSlot` (`20`) to the
holder. (`SlotChanged` currently serves only `mcrs_minecraft_client_gui` and has no server sync
consumer; observer `ClientboundSetEquipment` (`104`) belongs to `#115`.)

For break events (`ClientboundEntityEvent` `34`, status `47`, `49..=52`): `publish_player_signals`
writes `PlayerSignal` into `TileEvents` in `FixedPostUpdate`, `take_output` extracts
`OutboundPlayerPacket` into `output.client.packets` and `TileEvents::take()` into
`output.client.tiles`, and `dim_outputs::receive` currently runs **`deliver_all(world, packets)`
before `entity_replication::publish(world, dim, tiles)`**.

Sending break `entity_event` solely via `PlayerSignal` -> `TileEvents` would therefore arrive
**after** `ClientboundSetHealth`, `ClientboundDamageEvent`, and `ClientboundContainerSetSlot`,
violating `DUR-NET-002`. Fixing that seam requires either flushing pre-update break `TileEvents`
before `deliver_all` in `dim_outputs::receive` or queuing holder break `ClientboundEntityEvent`
ahead of `send_health` / `sync_menus` while preserving broadcast-once `EntityCodec::send_signals`
encoding (`to_owner` extended alongside `MobSignal::Hurt { .. }`) for observers.

## Open Prerequisites and Follow-Up Seams

- **Deduplicate `stack_view.rs`**: migrate `crates/mcrs_minecraft_model/src/stack_view.rs:111-135`
  (`is_damageable`, `max_damage`, `damage_value`, `is_damaged`, `next_damage_will_break`) onto shared
  `STK-DMG-001`..`008` helpers in `mcrs_minecraft_item_stack`.
- **Promote `Weapon` component**: move `Weapon` (`item_damage_per_attack`,
  `disable_blocking_for_seconds`) into `mcrs_minecraft_item_stack::for_each_builtin!` and stop
  skipping `DataComponentType::Weapon` in `skip_value.rs`.
- **Expose pre-reduction `raw_amount` and melee trigger in Layer 4**: add `raw_amount: f32` to
  `HitReport`/`Damaged` and add the accepted melee-hit durability message in `mcrs_minecraft_engine_damage`
  (emitted from `mcrs_minecraft_vanilla_player_attack` in `DamageSet::PacketReport`).
- **Break `entity_event` packet ordering & `#115`**: ensure holder `ClientboundEntityEvent` (`47`,
  `49..=52`) flushes before `block_update`, `damage_event`, `set_entity_data`, and `set_health`
  (`DUR-NET-002`), and wire observer `ClientboundSetEquipment` under `#115`.
- **Deferred Wave-2 triggers**: shield blocking (`minecraft:blocks_attacks`), shears/flint via
  `DurabilityExtraUse`, Mending repair, and non-player mob armour wear.
