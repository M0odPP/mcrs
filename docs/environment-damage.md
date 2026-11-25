# Environment damage: design

Design note for player environmental hazards: fall, fire and lava, drowning, freezing in powder snow, block contact, suffocation, void, and world border.
Behaviour comes from `docs/spec/environment-damage.md` (`ENV`) and `docs/spec/entity-damage.md` (`DMG-FALL`, `DMG-HOT`, `DMG-PLR`).
It feeds `PendingHits` (`docs/damage.md`) without modifying `apply_hits` semantics.

## 1. Classification of the state

| Value | Kind | Owner |
|-------|------|-------|
| `PlayerFileState::{fire, air, ticks_frozen, fall_distance}` | authoritative persisted truth (`Fire`, `Air`, `TicksFrozen`, `fall_distance`) | `mcrs_minecraft_engine::player_state` (`PlayerPersistencePlugin`); mutated by `mcrs_minecraft_vanilla_env_damage` via comparison-guarded writes (`set_if_neq`) |
| `EnvTickState { in_powder_snow, age_ticks, extinguish_sound_cooldown, suffocating, last_fall_y }` | transient non-persisted per-tick state (`SparseSet` component on `Player`) | `mcrs_minecraft_vanilla_env_damage` (`ENV-FRZ-002`, `ENV-FRZ-003`, `ENV-NET-002`, `ENV-WAL-001`, `ENV-ORD-001`) |
| `DimensionWorldBorder` (`WorldBorder` + `WorldBorderDamage { safe_zone, damage_per_block }`) `(new)` | authoritative dimension border bounds & damage parameters (defaults `5.0`, `0.2`) | `mcrs_minecraft_engine_world` (alongside `DimensionTypeConfig`); `/worldborder` & save hooks (`ENV-BRD-002`) |
| `AIR_SUPPLY = 1`, `SHARED_FLAG_ON_FIRE = 1 << 0` `(new)` | wire entity-data index and bitflag constants | `mcrs_minecraft_protocol_game::entity_data_index` |
| `PlayerEntries::{air_supply, ticks_frozen}` `(new)` | replication snapshot fields (`VarInt` at `1` and `7`; `fire > 0` sets `SHARED_FLAG_ON_FIRE`) | `mcrs_minecraft_replication::player_entries`; projected in `publish_pose` (`TickSet::Project`) |
| `DamageTags::{IS_DROWNING, IS_FREEZING}` `(new)` | baked damage-type tag bits | `mcrs_minecraft_damage`; baked by `bake_damage_types` |
| `ClientboundSound` `(new)` | wire sound packet (`sound`, `source`, position, `volume`, `pitch`, `seed`) | `mcrs_minecraft_protocol_game::clientbound` (`ENV-NET-002`) |
| `OutboundPlayerPacket::Sound` & `EntityEvent` `(new)` | direct & viewer packet delivery payloads (16-block sound listener filter; event `67` to viewers + self) | `mcrs_minecraft_engine::bus` (`PacketTarget::PlayerSet` / `SinglePlayer`) |
| `PendingEnvSound`, `PendingEntityEvent` `(new)` | Layer-5 -> Layer-4 projection messages for range-filtered sounds and drowning bubbles | `mcrs_minecraft_vanilla_env_damage` -> `TickSet::Project` sender in `mcrs_minecraft_engine` |
| `minecraft:powder_snow` modifier on `movement_speed` | derived transient attribute modifier (`ENV-FRZ-005`) | `AttributeInstances` via `AttributeSources` (`mcrs_minecraft_vanilla_attributes`, `TickSet::Project`) |

`PlayerFileState` already owns `fall_distance: f64`, `fire: i16`, `air: i16`, and `ticks_frozen: i32` and is already registered by `PlayerPersistencePlugin::build` via `player_state_keys()`.
Layer-5 hazard systems mutate `&mut PlayerFileState` directly without duplicating persisted fields.
Layer-4 `publish_pose` (`crates/mcrs_minecraft_engine/src/publish_pose.rs`) adds `Changed<PlayerFileState>` to `PoseInputsChanged` and reads `&PlayerFileState` directly into `PlayerEntries` with zero Layer-5 dependency, publishing one `ClientboundSetEntityData` diff to `TileEvents` (`PlayerData::Entries`) and `send_own_entity_state`.

## 2. Crate map

| Crate | Layer | Content |
|-------|-------|---------|
| `mcrs_minecraft_protocol_game` | 3, data | `AIR_SUPPLY = 1` `(new)`, `SHARED_FLAG_ON_FIRE = 1 << 0` `(new)` in `entity_data_index`; `ClientboundSound` `(new)` |
| `mcrs_minecraft_replication` | 3, data | `PlayerEntries::{air_supply, ticks_frozen}` `(new)` and `changed_from` diffs |
| `mcrs_minecraft_damage` (`no_std`) | 3, rules | `DamageTags::{IS_DROWNING, IS_FREEZING}` `(new)` alongside `IS_FIRE` and `IS_FALL` |
| `mcrs_minecraft_env_damage` (`no_std`, no Bevy, `(new)`) | 3, rules | pure hazard rules: `fall_damage`, `step_player_fall`, `step_fire`, `ignite`, `extinguish`, `step_air`, `step_freeze`, `powder_snow_speed_modifier`, `suffocation_eye_box`, `contact_damage`, `void_damage`, `border_damage` |
| `mcrs_minecraft_engine_world` | 4 | `DimensionWorldBorder` / `WorldBorderDamage` `(new)` on dimension entities alongside `DimensionTypeConfig` |
| `mcrs_minecraft_engine_damage` | 4 | `bake_damage_types` bakes `IS_DROWNING` and `IS_FREEZING`; `DamageSet::{Collect, Filter, Apply, Report}` |
| `mcrs_minecraft_engine` | 4 | `publish_pose` projects `PlayerFileState` into `PlayerEntries` in `TickSet::Project`; `OutboundPlayerPacket::{Sound, EntityEvent}` `(new)` delivery (16-block sound filter; tracking viewers + self for event `67`) |
| `mcrs_minecraft_vanilla_attributes` | 5 | `AttributeSources` set in `TickSet::Project` (`.before(OwnAttributesSet)`); hosts `sync_powder_snow_speed` (or receives it from Layer-5) |
| `mcrs_minecraft_vanilla_env_damage` `(new)` | 5 | `VanillaEnvDamagePlugin`: sparse player hazard systems feeding `PendingHits` in `DamageSet::Collect` and reading `Res<GameRules>` |
| `mcrs_minecraft_server` | 7 | registers `VanillaEnvDamagePlugin` per dimension (`MinecraftServerPlugins`); omitted by creative/build servers |

Keeping `mcrs_minecraft_env_damage` `#![no_std]` with zero Bevy lets headless bots, prediction checks, and unit tests share exact math (`f32`/`f64` order, `-20` player fire grace, `floor(-b * damage_per_block)`).

## 3. Pure kernel (`mcrs_minecraft_env_damage`, `(new)`)

All rules live as pure `#![no_std]` functions in `crates/mcrs_minecraft_env_damage` (`mcrs_minecraft_damage` does not provide `fall_damage`):

- **Fall (`step_player_fall`, `fall_damage`)**: accumulates downward displacement from verified movement and `ClientOnGround` (`PHY-FALL-003`), resets on upward displacement, water/fluid, climbing, or `FALL_DAMAGE_RESETTING`, and applies block fall multipliers (`hay_block`, `slime_block`, `DMG-FALL-002`, `DMG-FALL-004`).
- **Fire (`step_fire`, `ignite`, `extinguish`, `in_fire_contact`, `lava_contact`, `player_grace_reset`)**: updates `fire` (`-20` player grace), computes `on_fire` (`1.0`), `in_fire` (`1.0`/`2.0`), `lava` (`4.0`) hits, and flags `extinguish_fire` / `burn` sounds (`ENV-FIR-001`..`009`).
- **Air (`step_air`)**: decrements `air` when eyes are in water outside `bubble_column` (`-1`, resetting `<= -20` -> `0` with bubble event `67` and `drown` `2.0`) or refills `+4` up to `300` (`ENV-AIR-001`..`005`).
- **Freezing (`step_freeze`, `powder_snow_speed_modifier`)**: increments `ticks_frozen` in `powder_snow` up to `140` when not burning/spectator/wearing `freeze_immune_wearables`, thaws `-2` otherwise, emits `freeze` `1.0` when `ticks_frozen >= 140` and `age_ticks % 40 == 0`, and computes `-0.05_f32 * (min(frozen, 140) as f32 / 140.0_f32)` widened to `f64` (`ENV-FRZ-001`..`006`).
- **Spatial hazards (`suffocation_eye_box`, `contact_damage`, `void_damage`, `border_damage`)**: evaluates `0.8 * width` eye box (`ENV-WAL-001`), `cactus`/`campfire`/`sweet_berry_bush`/`magma_block` (`ENV-CTC-001`..`003`, `DMG-HOT-001`), `y < min_y - 64` `out_of_world` `4.0` (`ENV-VOID-001`), and `WorldBorder` + `WorldBorderDamage` `outside_border` (`ENV-BRD-001`).

## 4. Tick pipeline, lookup, and ECS constraints

- **Zero `&mut World` and `mcrs_lint` compliance**: every system uses typed `Populated`/`Query`, `Res`/`ResMut`, `WorldView`/`WorldViewMut`, and `MessageWriter` (`mcrs::exclusive_system`). All tick systems are gated (`Populated`, `resource_exists`) and guard writes with `set_if_neq` / comparison checks (`mcrs::missed_set_if_neq`, `mcrs::ungated_system`). Non-persisted per-tick flags (like `EnvTickState::suffocating`) live inside one `SparseSet` component rather than toggling unit marker components every tick (`mcrs::marker_toggle`).
- **Block/fluid lookup beyond `BlockFlags`**: existing `BlockFlags` (`mcrs_minecraft_physics::block_flags`) only exposes `SUFFOCATES`, `WATER`, `INSIDE_ACTION`, `STEP_ACTION`, and `FALL_DAMAGE_RESETTING` — it cannot distinguish `fire`, `soul_fire`, `lava`, `lava_cauldron`, `cactus`, `campfire`, `soul_campfire`, `magma_block`, `powder_snow`, `bubble_column`, or `sweet_berry_bush`. Hazard systems resolve block states along the player's shrunk box/eye/step position from `WorldView` against baked canonical block-id/tag tables (plus `BlockFlags::SUFFOCATES` + collision shapes for `in_wall`). `ENV-FIR-009` (`powder_snow` clearing by a burning player) submits `BlockSetRequest` (`WorldViewMut`) inside `TickSet::WorldWrite`.
- **`DamageSet::Collect` ordering and `GameRules`**: `PlayerVitalsPlugin` has no game-rule filter today, so `VanillaEnvDamagePlugin` reads `Res<GameRules>` (`fall_damage`, `drowning_damage`, `fire_damage`, `freeze_damage`) directly before `PendingHits::push` while still updating `PlayerFileState`. Systems run in deterministic `ENV-ORD-001` order using `WriterKey::Block { pos: YzxPos }` for block hazards and `WriterKey::Entity { key: EntityUuid }` with monotonically increasing hazard `sequence` numbers for player-tick hazards (`apply_hits` sorts by `(EntityUuid, WriterKey, tick, sequence)`):
  1. `collect_base_tick_hazards` (`on_fire` tick, `out_of_world`, `in_wall` setting `EnvTickState::suffocating` -> `outside_border`, `drown` + `PendingEntityEvent(67)`).
  2. `collect_player_fall_hits` (`ClientOnGround`, verified displacement, `PlayerFileState::fall_distance`, `fall` hits).
  3. `collect_block_and_freeze_hazards` (box/fluid scan -> `ENV-FRZ-002` freeze mark, `ENV-FRZ-006` frozen clear, `fire`/`lava` ignite + hits, `cactus`/`campfire`/`sweet_berry_bush`/`magma_block` hits, `ENV-FIR-009` `powder_snow` clear + `ENV-FIR-007` extinguish, `ENV-FIR-005` `-20` grace, `ENV-FRZ-003` freeze tick & thaw).
- **`TickSet::Project` (attributes, entity data, sounds, event `67`)**:
  - `sync_powder_snow_speed` runs in `AttributeSources` (`.in_set(TickSet::Project).before(OwnAttributesSet)`) reading `Populated<(&PlayerFileState, &mut AttributeInstances), Changed<PlayerFileState>>`, mutating via `bypass_change_detection().apply(Attribute::MovementSpeed, ...)` and calling `instances.set_changed()` only on `InstanceChange::Changed`.
  - `publish_pose` (`PoseSystems::Publish`) projects `PlayerFileState` into `PlayerEntries`; `Damaged` (`entity.generic.burn` on accepted `lava` hit) and `PendingEnvSound` (`entity.generic.extinguish_fire`) send `OutboundPlayerPacket::Sound` `(new)` to players within 16 blocks (`PacketTarget::PlayerSet`), and `PendingEntityEvent(67)` sends `OutboundPlayerPacket::EntityEvent` `(new)` to tracking viewers and the player (`TileEvents`/`MobSignal` are not used for sounds or event `67`).

## 5. Deferred mob and vehicle scope

Non-player hazards (`ENV-AIR-006`, `ENV-AIR-007`, `immune_to_freezing`, `freeze_hurts_extra_types`, fire-immune mob types, `mob_griefing`, `pointed_dripstone`) wait for mob entity-data replication, vehicle dismount, and mob save schemas without changing `mcrs_minecraft_env_damage` or `PendingHits`.
