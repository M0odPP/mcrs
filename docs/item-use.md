# Item use: start, hold, and release: design

Design note for server-side continuous item use, movement speed scaling, and `LIVING_FLAGS` metadata
replication. Behaviour comes from `docs/spec/item-use.md` (`USE`), `docs/spec/hunger.md` (`HNG`),
`docs/spec/entity-physics.md` (`PHY`), and `docs/spec/player-pose.md` (`POSE`).

## 1. Classification of the state

| Value | Kind | Owner |
|-------|------|-------|
| `UsingItem` (`SparseSet` component) on a player | authoritative truth (`hand`, `slot`, `elapsed_ticks`, `use_duration_ticks`, `speed_multiplier`, `can_sprint`) | `mcrs_minecraft_vanilla_item_use`; inserted on start, removed on release, slot change, death, or finish |
| `PlayerEntries::living_flags` | derived replication snapshot (`LIVING_FLAGS = 8`: bit `0` active use, bit `1` off hand, bit `2` spin attack) | `pose_entries` / `publish_pose` in `mcrs_minecraft_engine` via `set_if_neq` |
| `mcrs_minecraft_physics::ItemUse` (`PlayerState::item_use`) | derived movement input (`using`, `speed_multiplier`, `permits_sprint`) | synchronized from `UsingItem` added/changed/removed; read by `tick_player` (`0.2` default) |
| `StartUseItem`, `ReleaseUseItem`, `CarriedSlotChanged` | temporal facts (messages from packet observers) | emitted by `mcrs_minecraft_vanilla_item_use` observers; drained in `FixedUpdate` |
| `OwnPoseEntries` / `TileEvents` | per-tick replication state and outbox | `publish_pose` writes `ClientboundSetEntityData` (`LIVING_FLAGS = 8`) to self (`send_own_entity_state`) and `EntityEvent::Data` to `EntityCodec` |

`UsingItem` uses `#[component(storage = "SparseSet")]` so non-using players incur zero table-archetype
moves when starting, releasing, or cancelling item use.

## 2. Crate map

| Crate | Layer | Content |
|-------|-------|---------|
| `mcrs_minecraft_item_use` (`#![no_std]`) | 3, rules | Edition-neutral `ItemUseState`, `UseSpec`, `UseOutcome`, `living_flags_bits`, `start_use`, `tick_use`, `release_use`, `cancel_on_slot_change` |
| `mcrs_minecraft_item_stack`, `mcrs_minecraft_item` | 3, data | `Consumable`, `BlocksAttacks`, `UseEffects`, `Food`, `UseCooldown`, `UseRemainder` |
| `mcrs_minecraft_protocol_game` | 3, data | `ServerboundUseItem`, `ServerboundPlayerAction` (`PlayerAction::ReleaseUseItem`), `ServerboundSetCarriedItem`, `entity_data_index::LIVING_FLAGS` (`8`) |
| `mcrs_minecraft_replication` | 3, data | `PlayerEntries::living_flags` (`u8`) in `PartialEq` and `PlayerEntries::changed_from` emitting `MetadataEntry { index: LIVING_FLAGS, .. }` |
| `mcrs_minecraft_physics` | 3, rules | `ItemUse { using, speed_multiplier, permits_sprint }` (`0.2` default) in `tick_player` |
| `mcrs_minecraft_engine` | 4 | `pose_entries` / `publish_pose` folding `LIVING_FLAGS = 8` for self and observers; movement-check sync |
| `mcrs_minecraft_vanilla_item_use` | 5 | `ItemUsePlugin`, sparse `UsingItem`, `run_if`-gated packet ingest and `tick_using_items` |
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
     alive `Vitals`, non-`Spectator` `PlayerGameMode`, `Slots` / `Selected`, `Consumable` / `Food`
     (`can_always_eat` or `food < 20` or `Creative`), and `BlocksAttacks`; inserts sparse `UsingItem`
     and updates `ItemUse` (`0.2` multiplier unless overridden by `UseEffects`).
   - `handle_use_item_stops` (`.run_if(on_message::<ReleaseUseItem>.or(on_message::<CarriedSlotChanged>))`)
     removes `UsingItem` on `ReleaseUseItem` or hotbar slot change (`main` hand).
3. **`TickSet::Simulate`**:
   - `tick_using_items` (`.run_if(any_with_component::<UsingItem>)`, zero steady-state allocations)
     advances `elapsed_ticks` toward `use_duration_ticks`, validates the held stack, and on natural
     `Consumable` completion triggers eat finish (`entity_event` `9`, stack decrement, consume
     effects, and `UsingItem` removal).
   - Movement prediction reads `PlayerState::item_use` (`0.2x` forward/sideways input scale and sprint block).
4. **`TickSet::Project` (`PoseSystems::Publish`)**:
   - `publish_pose` (filtered on `Changed` inputs including `UsingItem` / `RemovedComponents<UsingItem>`)
     updates `PlayerEntries::living_flags` (`LIVING_FLAGS = 8`) via `set_if_neq` on `OwnPoseEntries`,
     sending `ClientboundSetEntityData` to the player (`send_own_entity_state`) and `TileEvents` so
     `EntityCodec` sends identical metadata changes once to all tracking observers.
