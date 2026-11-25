# Environment damage: design

Design note for Wave-1 player environmental hazards: fall, fire/lava, drowning, freezing, block contact, suffocation, void, and world border.
Behaviour comes from `docs/spec/environment-damage.md` (`ENV`) and `docs/spec/entity-damage.md` (`DMG-FALL`, `DMG-HOT`, `DMG-PLR`).
It extends the hit pipeline of `docs/damage.md` without modifying `apply_hits` semantics.

## 1. Classification of the state

| Value | Kind | Owner |
|-------|------|-------|
| `FireState { fire_ticks, extinguish_cooldown }` on a player | authoritative truth (`Fire` save field, player grace `-20`) | `mcrs_minecraft_vanilla_env_damage`; persisted via `PlayerFileState::fire` |
| `AirSupply(i16)` on a player | authoritative truth (`Air` save field, max `300`) | `mcrs_minecraft_vanilla_env_damage`; persisted via `PlayerFileState::air` |
| `FreezeState { ticks_frozen, in_powder_snow, age_ticks }` on a player | authoritative truth (`TicksFrozen` save field, `0..=140`) | `mcrs_minecraft_vanilla_env_damage`; persisted via `PlayerFileState::ticks_frozen` |
| `PlayerFallState { fall_distance, last_position }` on a player | authoritative truth from verified displacement and `ClientOnGround` (`PHY-FALL-003`) | `mcrs_minecraft_vanilla_env_damage` (`fall_distance` synced to `PlayerFileState`) |
| `WorldBorderDamage { buffer, damage_per_block }` on a dimension | authoritative truth (defaults `5.0`, `0.2`; paired with `WorldBorder`) | dimension state + save/command hooks (`ENV-BRD-002`) |
| `InWallThisTick` per-tick flag (component or bit) | suppresses border damage while suffocating (`ENV-WAL-001`) | set by `collect_block_hazard_hits`, read by `collect_void_and_border_hits` |
| `AIR_SUPPLY = 1`, `SHARED_FLAG_ON_FIRE = 1 << 0` | wire entity-data index and bitflag constants | `mcrs_minecraft_protocol_game::entity_data_index` |
| `PlayerEntries` fields (`shared_flags`, `air_supply`, `ticks_frozen`) | replication snapshot encoded once per change | `mcrs_minecraft_replication` (`changed_from`); published by `publish_pose` / `TickSet::Project` |
| `DamageTags::{IS_DROWNING, IS_FREEZING}` | baked damage-type tag bits | `mcrs_minecraft_damage`; baked by `bake_damage_types` |
| `minecraft:powder_snow` modifier on `movement_speed` | derived transient attribute modifier (`ENV-FRZ-005`) | `AttributeInstances` in `AttributeSources` (`set_changed` only on change) |

`PlayerEntries` gains `pub air_supply: i32` (default `300`, `VarInt` at `AIR_SUPPLY`) and `pub ticks_frozen: i32` (default `0`, `VarInt` at `TICKS_FROZEN`), plus `SHARED_FLAG_ON_FIRE` (`1 << 0`) in `shared_flags` (`SHARED_FLAGS = 0`).
`changed_from` emits each changed entry in one `ClientboundSetEntityData` buffer shared across viewers (`TileEvents` / `PlayerData::Entries`) and sent to the own player.

## 2. Crate map

| Crate | Layer | Content |
|-------|-------|---------|
| `mcrs_minecraft_protocol_game` | 3, data | `AIR_SUPPLY = 1`, `SHARED_FLAG_ON_FIRE = 1 << 0` in `entity_data_index` |
| `mcrs_minecraft_replication` | 3, data | `PlayerEntries::{air_supply, ticks_frozen}` and `changed_from` diffs |
| `mcrs_minecraft_damage` (`no_std`) | 3, rules | `DamageTags::{IS_DROWNING, IS_FREEZING}` alongside `IS_FIRE` and `IS_FALL` |
| `mcrs_minecraft_env_damage` (`no_std`, no Bevy) | 3, rules | pure hazard rules: `FireState`, `AirSupply`, `FreezeState`, `FallTracker`, suffocation/contact/void/border checks, `powder_snow_speed_modifier` |
| `mcrs_minecraft_engine_damage` | 4 | `bake_damage_types` bakes `IS_DROWNING` and `IS_FREEZING`; `DamageSet::{Collect, Filter, Apply, Report}` |
| `mcrs_minecraft_engine` | 4 | `PlayerEntries` projection in `TickSet::Project`; `ClientboundEntityEvent` (`67`) and `ClientboundSound` forwarding via `OutboundPlayerPacket` / `TileEvents` |
| `mcrs_minecraft_vanilla_env_damage` (new) | 5 | `VanillaEnvDamagePlugin`: sparse/parallel player hazard systems feeding `PendingHits` in `DamageSet::Collect` |
| `mcrs_minecraft_server` | 7 | registers `VanillaEnvDamagePlugin` per dimension (`MinecraftServerPlugins`); omitted by creative/build servers |

Keeping `mcrs_minecraft_env_damage` `#![no_std]` with zero Bevy lets headless bots, prediction checks, and unit tests share exact math (`f32`/`f64` order, `-20` player grace, `floor(-b * damage_per_block)`).

## 3. Pure kernel (`mcrs_minecraft_env_damage`)

All hazard rules are pure functions over plain inputs and small state structs:

- **Fire (`step_fire`, `ignite`, `extinguish`, `in_fire_contact`, `lava_contact`, `player_grace_reset`)**: updates `fire_ticks` (`-20` grace for players), returns hit intents (`on_fire` `1.0`, `in_fire` `1.0`/`2.0`, `lava` `4.0`) and `extinguish_fire` / `burn` sound triggers (`ENV-FIR-001`..`007`, `ENV-NET-002`).
- **Air (`step_air`)**: decrements `air` when eye is in water outside `bubble_column` (`-1`, emits bubble `entity_event(67)` and `drown` `2.0` at `<= -20` -> `0`) or refills `+4` up to `300` (`ENV-AIR-001`..`005`).
- **Freezing (`step_freeze`, `powder_snow_speed_modifier`)**: increments `ticks_frozen` in `powder_snow` up to `140` when not burning/spectator/wearing `freeze_immune_wearables`, thaws by `-2` otherwise, emits `freeze` `1.0`/`1.5` when `ticks_frozen >= 140` and `age % 40 == 0`, and computes `-0.05_f32 * (min(frozen, 140) as f32 / 140.0_f32)` widened to `f64` (`ENV-FRZ-001`..`006`).
- **Fall (`step_player_fall`)**: accumulates downward displacement from verified movement and `ClientOnGround` (`PHY-FALL-003`), resets on upward motion/fluid/climbing/`fall_damage_resetting`, and evaluates block fall multiplier (`DMG-FALL-002`, `DMG-FALL-004`) via `mcrs_minecraft_damage::fall_damage`.
- **Spatial hazards (`suffocation_eye_box`, `contact_damage`, `void_damage`, `border_damage`)**: computes `0.8 * width` eye box (`ENV-WAL-001`), `cactus`/`campfire`/`sweet_berry_bush`/`magma_block` hits (`ENV-CTC-001`..`003`, `DMG-HOT-001`), `y < min_y - 64` `out_of_world` `4.0` (`ENV-VOID-001`), and `outside_border` (`ENV-BRD-001`).

## 4. Tick pipeline and ECS constraints

Every system in `mcrs_minecraft_vanilla_env_damage` uses typed `Query`/`Populated`, `Res`/`ResMut`, `WorldView`, and `MessageWriter` parameters — **zero `&mut World`** (`mcrs::exclusive_system`). All systems obey `mcrs_lint`:

- **Gated and sparse execution**: hazard tick systems use `Populated<...>` or `run_if(...)` (such as `resource_exists::<PhysicsTables>`) and write components with `set_if_neq` (`mcrs::missed_set_if_neq`, `mcrs::ungated_system`). State values like `FireState`, `AirSupply`, and `FreezeState` live as components rather than toggling unit markers every tick (`mcrs::marker_toggle`).
- **Split, parallel-friendly collection in `DamageSet::Collect`**:
  1. `collect_player_fall_hits` reads `Transform`, `ClientOnGround`, `PlayerFallState`, `AttributeInstances`, `WorldView`, and `PhysicsTables`, queuing `fall` hits into `PendingHits`.
  2. `collect_block_hazard_hits` scans each player's shrunk AABB/eye box against `WorldView` + `PhysicsTables` (`BlockFlags::{SUFFOCATES, INSIDE_ACTION, STEP_ACTION}`), updating `FireState`/`FreezeState` and pushing `in_wall`, `in_fire`, `lava`, `cactus`, `campfire`, `sweet_berry_bush`, and `hot_floor` requests. It records a per-tick `InWallThisTick` flag/bit so border damage skips suffocating players (`ENV-WAL-001`, `ENV-BRD-001`).
  3. `collect_air_hits` updates `AirSupply`, emits `ClientboundEntityEvent` (`67`) when `air` resets from `-20` to `0`, and queues `drown` hits.
  4. `collect_fire_and_freeze_ticks` advances burning (`on_fire`) and freezing (`freeze`) timers and queues hits; `sync_powder_snow_speed` updates `minecraft:powder_snow` in `AttributeSources` only on `Changed<FreezeState>`.
  5. `collect_void_and_border_hits` evaluates `DimensionTypeConfig::min_y` and `WorldBorderDamage`, queuing `out_of_world` and `outside_border`.
- **Deterministic merge and game-rule filtering**: systems with disjoint mutable components execute in parallel (or push via thread-local buffers into `PendingHits` with fixed per-hazard `WriterKey` and sequence numbers). `DamageSet::Filter` (or `PlayerVitalsPlugin` / env filter) rejects player hits when `GameRules` has `fall_damage`, `fire_damage`, `drowning_damage`, or `freeze_damage` off (`DMG-PLR-001`), and `DamageSet::Apply` (`apply_hits`) sorts by `(EntityUuid, WriterKey, tick, sequence)` so results never depend on thread scheduling.
- **Projection (`TickSet::Project`)**: `publish_pose` reads `FireState`, `AirSupply`, and `FreezeState` into `PlayerEntries` via `set_if_neq`; `Damaged` triggers `ClientboundSound` (`entity.generic.burn` on accepted `lava`, `entity.generic.extinguish_fire` on extinguish).

## 5. Wave-1 scope and deferred work

Wave 1 targets players (`#101`, `#108`, `#109`, `#110`, `#262`) as a standalone Layer-5 plugin.
Mob environmental hazards (`ENV-AIR-006`, `ENV-AIR-007`, `immune_to_freezing`, `freeze_hurts_extra_types`, fire-immune mobs, `pointed_dripstone` `stalagmite`) and mob persistence wait for mob entity data replication and save schemas without changing the `mcrs_minecraft_env_damage` kernel or `PendingHits` pipeline.
