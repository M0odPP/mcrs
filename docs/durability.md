# Item Durability Design Note (`#118`)

Wave-1 player item durability spans three layers and implements
[docs/spec/durability.md](spec/durability.md) plus `STK-DMG-001`..`STK-DMG-008` in
[docs/spec/item-stack.md](spec/item-stack.md).

## State Classification

| State | Kind | Owner / Lifetime |
| :--- | :--- | :--- |
| `ItemStack` (`kind`, `count`, `patch`) | Authoritative truth | `mcrs_minecraft_item_stack` (`PatchEntry` holds `Damage`, `MaxDamage`, `Unbreakable`, `Tool`, `Equippable`, `Weapon`, `Enchantments`) |
| Stack entity `Effective<T>`, `Stacks`, `InSlot`, `Equipped` | Materialized derived | `mcrs_minecraft_engine_item`; synced from `ItemStack` patches |
| `PlayerGameMode`, `Blocks` hardness, `DamageTypeRules` | Authoritative truth | `mcrs_minecraft_game_mode`, `mcrs_minecraft_block`, `mcrs_minecraft_damage` |
| `BlockBroken`, `Damaged`, melee hit event, `DurabilityExtraUse` | Temporal tick fact | Emitted in `TickSet::Simulate`; consumed in the same tick |
| `SlotChanged`, `PlayerSignal` / `MobSignal` via `TileEvents` | Temporal output fact | Emitted on mutation/break; flushed in `TickSet::Output` |

No long-lived durability accumulator component exists on players or items. Wear resolves
immediately within the tick that caused it.

## Layer and Crate Map

```text
Layer 1  mcrs_minecraft_item_stack (#![no_std])   -- STK-DMG-001..008 stack helpers + Weapon promotion
Layer 3  mcrs_minecraft_durability  (#![no_std])   -- pure wear formulas, Unbreaking RNG, SlotBreakEvent
Layer 5  mcrs_minecraft_vanilla_durability        -- sparse event-driven plugin + mcrs:durability_extra_use
Layer 7  mcrs_minecraft_server                    -- registers VanillaDurabilityPlugin per dimension
```

This layout preserves the strict layer DAG in `AGENTS.md`: `mcrs_minecraft_item_table` already
depends on `mcrs_minecraft_item_stack`, Layer 3 never depends on ECS, world, or network crates,
and Layer 5 owns all queries, entity commands, and `TileEvents` emission.

## 1. Stack-Level API (`mcrs_minecraft_item_stack`)

`ItemStack` already parses and patches `MaxDamage` (`>= 1`), `Damage` (`>= 0`), `Unbreakable`,
`Tool`, `Equippable`, and `Enchantments` via `for_each_builtin!`.

Proposed `STK-DMG-001`..`STK-DMG-008` helpers on `mcrs_minecraft_item_stack::ItemStack` take a
lightweight effective-defaults lookup trait implemented in `mcrs_minecraft_item_table` for
`Prototype` (keeping `mcrs_minecraft_item_table -> mcrs_minecraft_item_stack` acyclic), plus a
proposed `BreakBehavior { BreakAtMax, ClampBelowMax }` enum instead of a bare `bool`:

- `is_damageable`: true when effective `MaxDamage` and `Damage` exist and `Unbreakable` is absent
  (`STK-DMG-001`).
- `damage_value`: effective `Damage` clamped to `0..=max_damage` (`STK-DMG-002`).
- `set_damage`: clamps `value` to `0..=max_damage` and writes `Damage` patch (`STK-DMG-003`).
- `is_damaged`, `is_broken`, `will_break_on_next_damage`: (`STK-DMG-004`, `STK-DMG-008`).
- `apply_damage`: executes `STK-DMG-005`..`STK-DMG-007`; `BreakAtMax` decrements `count` on
  reaching `max_damage` and returns a `BreakOutcome`, whereas `ClampBelowMax` clamps to
  `max_damage - 1` (`STK-DMG-007`).

### ECS `Damage` Write Rule

On stack entities inside `mcrs_minecraft_engine_item`, `Effective<Damage>` holds `0` when no
`Damage` patch exists on the stack archetype. Mutating via `Query<&mut Damage>` misses fresh
items (such as `golden_pickaxe` or `iron_sword` at `damage = 0`).

Write mutations in Layer 5 with this exact rule:

1. If `Damage` is present on the stack entity (`Query<&mut Damage>` matches), update it in place and
   call `commands.entity(stack_entity).touch()` so `mcrs_minecraft_engine_item` detects the edit and
   emits `SlotChanged`.
2. Otherwise, call `commands.entity(stack_entity).set(Damage(new_damage))` (`StackCommandsExt::set`).
3. If `new_damage >= max_damage`, call `commands.entity(stack_entity).consume(1)`
   (`StackCommandsExt::consume`) and emit the slot break signal.

## 2. `#![no_std]` Kernel (`mcrs_minecraft_durability`)

Proposed `mcrs_minecraft_durability` depends only on `mcrs_minecraft_item_stack`,
`mcrs_minecraft_damage`, and `mcrs_minecraft_entity_key`:

- Proposed `WearKind` enum `{ Tool, Armour }`: selects Unbreaking retention probability without a
  bare `bool` (`1.0 / (level + 1) as f64` for `Tool`; `0.6 + 0.4 / (level + 1) as f64` for `Armour`).
- Proposed `roll_unbreaking(amount: i32, unbreaking_level: i32, kind: WearKind, stream: &mut KeyedStream) -> i32`:
  returns `amount` unchanged when `unbreaking_level <= 0`; otherwise evaluates `amount` draws via
  `KeyedStream::unit(draw)` (`[0.0, 1.0)`) and counts retained points.
- Proposed `block_break_wear(tool: &Tool, block_hardness: f32) -> i32` (`damage_item_on_block_break`):
  returns `0` when `block_hardness <= 0.0`, else `tool.damage_per_block.max(0)`.
- Proposed `attack_wear(weapon: &Weapon) -> i32`: returns `weapon.item_damage_per_attack`.
- Proposed `armour_hit_wear(raw_amount: f32, equ: &Equippable, tags: DamageTags) -> i32`: returns
  `0` when `!equ.damage_on_hurt`, `tags.contains(DamageTags::BYPASSES_ARMOR)`, or
  `raw_amount <= 0.0`; else `(raw_amount / 4.0).floor().max(1.0) as i32`.
- Proposed `SlotBreakEvent` enum with exact wire discriminants (`MainHand = 47`, `OffHand = 48`,
  `Feet = 49`, `Legs = 50`, `Chest = 51`, `Head = 52`).

## 3. Sparse Layer-5 Plugin (`mcrs_minecraft_vanilla_durability`)

Proposed `VanillaDurabilityPlugin` (registered on `MinecraftServerPlugins` via
`add_dimension_plugin`) has zero dense per-player or per-item `tick` loops. It processes four
sparse message streams in `TickSet::Simulate` (after block-break, attack, and damage systems,
before `TickSet::WorldWrite`):

1. **Block break (`on_block_broken`)**: reads `BlockBroken`, skips `PlayerGameMode(GameMode::Creative)`,
   resolves `Blocks::state(msg.state).hardness` and `breaker`'s `Selected` stack (`Effective<Tool>`),
   applies `block_break_wear`, and sends `SlotBreakEvent::MainHand` (`47`) on break.
2. **Melee attack (`on_melee_attack`)**: reads accepted primary hit events from
   `mcrs_minecraft_vanilla_player_attack` (`ATK-DMG-004`), skips `PlayerGameMode(GameMode::Creative)`,
   inspects the attacker's `Selected` stack (`Effective<Weapon>`), and applies `attack_wear`.
3. **Armour hit (`on_entity_damaged`)**: reads `Damaged` (`mcrs_minecraft_engine_damage`), skips
   rejected hits and `PlayerGameMode(GameMode::Creative)`, reads `DamageTypeRules::tags`, and
   iterates `ArmourSlot::{Feet, Legs, Chest, Head}` (`36..=39`) via `Equipped` / `Slots`. Each
   eligible armour stack applies `armour_hit_wear` independently and emits its `SlotBreakEvent`
   (`49..=52`) on break.
4. **Extension hook (`DurabilityExtraUse`)**: proposed message representing `mcrs:durability_extra_use`
   (`entity: Entity`, slot selector, `base_amount: i32`) for Wave-2 callers such as shields and
   tools; runs the shared `apply_slot_wear` helper.

### Broadcast-Once Wire Emission

When a stack breaks, the plugin emits `PlayerSignal::new(holder, MobSignal::ItemBreak(event))`
(proposed `MobSignal` variant carrying `SlotBreakEvent`) into `TileEvents`
(`EntityEvent::Data(PlayerData::Signal(_))`).

In `mcrs_minecraft_replication::EntityCodec::send_signals`, `MobSignal` is already encoded into
`Bytes` once per frame and sent to every tracking client, while `to_owner` includes the holder
client for `MobSignal::Hurt { .. }`. Extending `to_owner` to include `MobSignal::ItemBreak(_)`
delivers `ClientboundEntityEvent` (`34`, status `47..=52`) once to both holder and observers
before world/damage updates, while `SlotChanged` drives `minecraft:container_set_slot` (`20`) and
`minecraft:set_equipment` (`104`).

## Open Prerequisites and Follow-Up Seams

- **Promote `Weapon` component**: move `Weapon` (`item_damage_per_attack`,
  `disable_blocking_for_seconds`) from `mcrs_minecraft_item::component::simple` into
  `mcrs_minecraft_item_stack::for_each_builtin!` and remove `DataComponentType::Weapon` from
  `skip_value.rs`.
- **Expose pre-reduction hit amount**: `HitReport` and `Damaged` currently carry only
  post-reduction fields (`armor`, `absorption`, `health`, `died`). Add `raw_amount: f32` to
  `Damaged` (or `HitReport`) so `on_entity_damaged` computes `floor(raw_amount / 4)`.
- **Emit melee hit trigger**: emit an accepted melee hit message from
  `mcrs_minecraft_vanilla_player_attack` at the `ATK-DMG-004` hook point after `DamageSet::PacketReport`.
- **Deferred Wave-2 consumers**: shield blocking (`minecraft:blocks_attacks`), shear/flint-and-steel
  extra uses via `DurabilityExtraUse`, Mending repair, and non-player mob armour wear.
