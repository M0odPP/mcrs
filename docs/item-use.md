# Item use: start, hold, and release: design

Design note for server-side continuous item use, movement speed scaling, and `LIVING_FLAGS` metadata
replication. Behaviour comes from `docs/spec/item-use.md` (`USE`), `docs/spec/hunger.md` (`HNG`),
`docs/spec/entity-physics.md` (`PHY`), and `docs/spec/player-pose.md` (`POSE`).

## 1. Classification of the state

| Value | Kind | Owner |
|-------|------|-------|
| `UsingItem` (`SparseSet` component) in `mcrs_minecraft_engine` | authoritative truth (`hand`, `slot`, `elapsed_ticks`, `use_duration_ticks`, `speed_multiplier`, `can_sprint`) | inserted/updated/removed by `mcrs_minecraft_vanilla_item_use`; read by Layer-4 systems |
| `LivingFlags(u8)` on `Player` + `PlayerEntries::living_flags` | derived replication truth (`LIVING_FLAGS = 8`: bit `0` active use, bit `1` last active hand `0`/`1`, bit `2` spin attack) | maintained in `mcrs_minecraft_engine` via `set_if_neq` (`0` bit cleared on stop while preserving bit `1`) |
| `mcrs_minecraft_physics::ItemUse` (`PlayerState::item_use`) | derived movement input (`using`, `speed_multiplier`, `permits_sprint`) | synced from Layer-4 `UsingItem`; read by `tick_player` (`0.2` default) |
| `StartUseItem`, `ReleaseUseItem`, `CarriedSlotChanged` | temporal facts (messages from packet observers) | emitted by `mcrs_minecraft_vanilla_item_use` observers; drained in `FixedUpdate` |
| `OwnPoseEntries` / `TileEvents` | per-tick replication state and outbox | `publish_pose` writes `ClientboundSetEntityData` (`LIVING_FLAGS = 8`) to self (`send_own_entity_state`) and `EntityEvent::Data` to `EntityCodec` |

`UsingItem` lives in Layer-4 `mcrs_minecraft_engine` with `#[component(storage = "SparseSet")]` so
non-using players incur zero table-archetype moves and Layer-4 systems (`publish_pose`, movement
checks) read it without depending on Layer 5.

## 2. Crate map

| Crate | Layer | Content |
|-------|-------|---------|
| `mcrs_minecraft_item_use` (`#![no_std]`) | 3, rules | Edition-neutral `ItemUseState`, `UseSpec`, `UseOutcome`, `living_flags_bits`, `start_use`, `tick_use`, `release_use`, `cancel_on_slot_change` |
| `mcrs_minecraft_item_stack` | 3, data | Adds typed `#![no_std]` decoders for `Consumable`, `BlocksAttacks`, and `UseEffects` alongside existing `Food` (currently skipped in `skip_value.rs`; avoids legacy `mcrs_minecraft_item`) |
| `mcrs_minecraft_protocol_game` | 3, data | `ServerboundUseItem`, `ServerboundPlayerAction` (`PlayerAction::ReleaseUseItem`), `ServerboundSetCarriedItem`, `entity_data_index::LIVING_FLAGS` (`8`) |
| `mcrs_minecraft_replication` | 3, data | `PlayerEntries::living_flags` (`u8`) in `PartialEq` and `PlayerEntries::changed_from` emitting `MetadataEntry { index: LIVING_FLAGS, .. }` |
| `mcrs_minecraft_physics` | 3, rules | `ItemUse { using, speed_multiplier, permits_sprint }` (`0.2` default) in `tick_player` |
| `mcrs_minecraft_move_predict` | 3, rules | New `Change::ItemUse` variant when `mcrs_minecraft_engine::movement` wires `Prediction` |
| `mcrs_minecraft_engine` | 4 | Layer-4 `UsingItem` (`SparseSet`) and `LivingFlags(u8)`; `pose_entries` / `publish_pose` folding `LIVING_FLAGS = 8` for self and observers |
| `mcrs_minecraft_vanilla_item_use` | 5 | `VanillaItemUsePlugin`, `run_if`-gated packet ingest and `tick_using_items` driving `UsingItem` |
| `mcrs_minecraft_server` | 7 | `VanillaItemUsePlugin` in `MinecraftServerPlugins` |

Reason for `mcrs_minecraft_item_use`: bots, proxies, movement validation, and Bedrock adapters share
`use_duration_ticks`, `living_flags_bits`, and `0.2x` speed rules without taking Bevy, tokio, or
engine registries.

## 3. Use cases

| User | Takes | Does not take |
|------|-------|---------------|
| proxy, replay viewer | `mcrs_minecraft_protocol_game`, `mcrs_minecraft_item_use` | Bevy, physics, server engine |
| minigame server | `mcrs_minecraft_engine`; inserts `UsingItem` or omits `VanillaItemUsePlugin` | vanilla food/shield start rules |
| headless bot | `mcrs_minecraft_item_use`, `mcrs_minecraft_physics` | rendering, server engine |
| Bedrock edge | translates input transactions into `mcrs_minecraft_item_use` transitions | Java wire types |

## 4. Server tick and ECS gating

1. **Ingest (`On<Received<_>>` observers)**: `ServerboundUseItem`, `PlayerAction::ReleaseUseItem`,
   and `ServerboundSetCarriedItem` write lightweight messages without touching world tables.
2. **`TickSet::ClientInput`**:
   - `handle_use_item_starts` (`.run_if(on_message::<StartUseItem>)`) checks `UnloadedClient`,
     alive `Vitals`, non-`Spectator` `PlayerGameMode`, no active `UsingItem`, `Slots` / `Selected`,
     `Consumable` / `Food`, and `BlocksAttacks`; inserts sparse `UsingItem` and updates `LivingFlags`
     (setting bit `0` and bit `1` for hand) and `ItemUse`.
   - `handle_use_item_stops` (`.run_if(on_message::<ReleaseUseItem>.or(on_message::<CarriedSlotChanged>))`)
     removes `UsingItem` on `ReleaseUseItem` or `main`-hand hotbar slot change and clears bit `0` of
     `LivingFlags` via `set_if_neq` while preserving bit `1`.
3. **`TickSet::Simulate`**:
   - `tick_using_items` (`.run_if(any_with_component::<UsingItem>)`, zero steady-state allocations)
     advances `elapsed_ticks` toward `use_duration_ticks`, cancels on death/spectator/stack change,
     and on natural `Consumable` completion triggers eat finish (`entity_event` `9`, stack
     decrement, consume effects, `UsingItem` removal, and `LivingFlags` bit `0` clear).
   - Movement integration is new Layer-4 wiring: `crates/mcrs_minecraft_engine/src/movement.rs` does
     not yet drive `mcrs_minecraft_move_predict::Prediction`, so wiring `Prediction` will feed
     `UsingItem` into `PlayerState::item_use` (`0.2x` input scale and sprint block) via a new
     `Change::ItemUse` variant.
4. **`TickSet::Project` (`PoseSystems::Publish`)**:
   - `LivingFlags` (or a separate `RemovedComponents<UsingItem>` `SystemParam` alongside `publish_pose`,
     since `RemovedComponents` is not a `Query` filter) feeds `PoseInputsChanged` (`Changed<LivingFlags>`).
   - `publish_pose` folds `LivingFlags` into `PlayerEntries::living_flags` (`LIVING_FLAGS = 8`) via
     `set_if_neq` on `OwnPoseEntries`, sending `ClientboundSetEntityData` to the player
     (`send_own_entity_state`) and `TileEvents` so `EntityCodec` sends identical metadata once to
     all tracking observers.
