# Hunger, exhaustion, natural regeneration and starvation: design

Design note for server-side player hunger state, exhaustion accumulation, natural and peaceful
regeneration, starvation damage and client health/food synchronization (#261). Behaviour comes from
`docs/spec/hunger.md` (HNG-STA, HNG-EXH, HNG-TICK, HNG-REG, HNG-STV, HNG-NET, HNG-SAV),
`docs/spec/entity-damage.md` (DMG-NET-005) and `docs/damage.md`.

Wave-1 scope covers the server hunger tick, sprint/jump/attack/block-break/damage/`hunger`-effect
exhaustion, natural regeneration, peaceful regeneration, starvation hits, NBT persistence and
`set_health` wire sync. Eating (`HNG-EAT`), fluid/climbable movement exhaustion (`HNG-EXH-002`)
and the `apply_exhaustion` enchantment effect stay deferred.

## 1. Classification of the state

| Value | Kind | Owner |
|-------|------|-------|
| `HungerState` (`mcrs_minecraft_hunger`) | pure state value: `food_level`, `saturation`, `exhaustion`, `tick_timer` | evaluated by `mcrs_minecraft_vanilla_hunger` systems; synced to/from `PlayerFileState` |
| `PlayerFileState` (`mcrs_minecraft_engine`) | authoritative persisted truth for `foodLevel`, `foodTickTimer`, `foodSaturationLevel`, `foodExhaustionLevel` and `active_effects` | `PlayerPersistencePlugin` on load/save/travel; `respawn_players` on respawn; `mcrs_minecraft_vanilla_hunger` via `set_if_neq` |
| `Vitals` (`mcrs_minecraft_damage`) | authoritative entity health | `apply_hits` on damage/starvation; `mcrs_minecraft_vanilla_hunger` writes `health` via `set_if_neq` on natural/peaceful heal |
| `PendingHits` (`mcrs_minecraft_engine_damage`) | per-tick hit work queue | `mcrs_minecraft_vanilla_hunger` appends `minecraft:starve` (`1.0`) in `DamageSet::Collect`; `apply_hits` drains in `DamageSet::Apply` |
| `SentHealth` (`mcrs_minecraft_engine_player_vitals`) | wire deduplication cache (`health`, `food`, `saturation == 0.0`) | `send_health` in `TickSet::Project` via `set_if_neq`; removed by `respawn_players` and absent after dimension arrival |
| `FoodLevel` (`mcrs_minecraft_client_world`) | client replica (`food`, `saturation`) | `on_set_health` (`ClientboundSetHealth`) and `on_respawn` via `set_if_neq` |
| food bar HUD (`mcrs_minecraft_client_gui`) | derived UI from `FoodLevel` | `HealthHudPlugin` |

## 2. Crate map

| Crate | Layer | Content |
|-------|-------|---------|
| `mcrs_minecraft_hunger` (`#![no_std]`, new) | 3, rules | `HungerState`, `HungerDifficulty`, `HungerTickInput`, `HungerTickOutput`, pure exhaustion formulas (`sprint_move_exhaustion`, `jump_exhaustion`, `hunger_effect_exhaustion`, `ATTACK_EXHAUSTION`, `HARVEST_BREAK_EXHAUSTION`) and `HungerState::tick` |
| `mcrs_minecraft_damage` | 3, rules | `Vitals`, `Hit`, `DamageSource`, `DamageTypeRules` (`crates/mcrs_minecraft_damage/src/damage_type_rules.rs`) |
| `mcrs_minecraft_engine_world` | 4 | `Player`, `ColumnIndex`, `SectionStage`, `GameRules`, `ServerDifficulty` resource (wrapping `mcrs_minecraft_save::Difficulty`) |
| `mcrs_minecraft_engine` | 4 | `PlayerFileState` (`food_level`, `food_tick_timer`, `food_saturation_level`, `food_exhaustion_level`, `active_effects`), `PlayerPersistencePlugin`, `PlayerSprinting`, `ClientOnGround`, player lived tick counter (`HNG-REG-004`) |
| `mcrs_minecraft_engine_damage` | 4 | `DamageSet::{PacketCollect..PacketReport, Collect..Report}`, `PendingHits`, `DamageRequest`, `Damaged`, `bake_damage_types` |
| `mcrs_minecraft_engine_player_vitals` | 4 | `PlayerVitalsPlugin`, `send_health` (`SentHealth::set_if_neq` -> `ClientboundSetHealth`), `respawn_players` |
| `mcrs_minecraft_vanilla_hunger` (new) | 5 | `VanillaHungerPlugin`: exhaustion collectors, `PlayerFileState` <-> `HungerState` bridge, `Vitals` heal sync, `PendingHits` starvation emitter in `DamageSet::Collect` |
| `mcrs_minecraft_server` | 7 | adds `VanillaHungerPlugin` as a dimension plugin in `MinecraftServerPlugins` |
| `mcrs_minecraft_client_world` | 4 | `FoodLevel`, `on_set_health` (`set_health` -> `Vitals` + `FoodLevel` sync via `set_if_neq`) |

Reason for `#![no_std]` `mcrs_minecraft_hunger`: headless bots, unit tests and Layer-5 gameplay
plugins share the exact drain/regen/starve arithmetic and f32 exhaustion math without pulling Bevy
or server persistence.

## 3. The `#![no_std]` state machine (`mcrs_minecraft_hunger`)

`HungerState` mirrors the four persisted fields (`food_level: i32`, `saturation: f32`,
`exhaustion: f32`, `tick_timer: i32`) with default `(20, 5.0, 0.0, 0)` (`HNG-STA-001`):

- `add_exhaustion(&mut self, amount: f32)`: ignored when `invulnerable` (`HNG-EXH-001`); adds
  positive finite amounts and clamps `exhaustion` to `[0.0, 40.0]` (`HNG-STA-002`).
- `tick(&mut self, input: HungerTickInput) -> HungerTickOutput` runs `HNG-TICK-002` atomically:
  1. **Peaceful regeneration (`HNG-REG-004`, `HNG-REG-005`)**: when `difficulty == Peaceful` and
     `natural_regeneration` is true, every 20 lived ticks heals `+1.0` (clamped to `max_health`, no
     exhaustion cost) and adds `+1.0` to `saturation` below `20.0`; every 10 lived ticks adds `+1`
     to `food_level` below `20`.
  2. **Hunger drain (`HNG-TICK-001` step 1)**: if `exhaustion > 4.0`, subtracts `4.0` once and
     either reduces `saturation` by `1.0` (clamped at `0.0`) or, when `difficulty != Peaceful`,
     reduces `food_level` by `1` (clamped at `0`).
  3. **Branch selection (`HNG-TICK-001` step 2)**: picks the first matching branch or resets
     `tick_timer = 0`:
     - *Saturated heal (`HNG-REG-001`)*: `natural_regeneration && saturation > 0.0 && food_level >= 20 && 0.0 < health < max_health`.
       Increments `tick_timer`; at `10`, resets timer, takes `s = saturation.min(6.0)`, adds `s / 6.0`
       heal and adds `s` exhaustion.
     - *Normal heal (`HNG-REG-002`)*: `natural_regeneration && food_level >= 18 && 0.0 < health < max_health`.
       Increments `tick_timer`; at `80`, resets timer, adds `1.0` heal and `6.0` exhaustion.
     - *Starvation (`HNG-STV-001`)*: `food_level <= 0` (independent of `natural_regeneration`).
       Increments `tick_timer`; at `80`, resets timer and signals `starve = true` when
       `health > 10.0 || difficulty == Hard || (health > 1.0 && difficulty == Normal)`.
- `HungerTickOutput { next_health: Option<f32>, starve: bool }` keeps `next_health` clamped to
  `max_health` (`HNG-REG-003`).

## 4. Layer-5 ECS wiring and sync (`mcrs_minecraft_vanilla_hunger`)

1. **Exhaustion collection**:
   - *Packet melee attacks (`HNG-EXH-004`)*: in `DamageSet::PacketReport`, reads `Damaged` messages
     with `HitOutcome != Rejected` and `damage_type == minecraft:player_attack`, adding `0.1` to the
     direct player attacker when not creative/spectator.
   - *Moves & jumps (`HNG-EXH-002`, `HNG-EXH-003`)*: in `TickSet::Simulate` (after
     `TickSet::ClientInput`), reads accepted player displacements and `on_ground` transitions (`true -> false`
     with `dy > 0.0`), adding `0.2` (sprint jump) or `0.05` (non-sprint jump) and
     `0.1 * round(horiz * 100.0) * 0.01` when on ground and `PlayerSprinting(true)`.
   - *Block harvests (`HNG-EXH-005`)*: in `TickSet::Simulate`, gated on `on_message::<BlockBroken>`,
     adds `0.005` when `harvests` is true.
   - *Damage taken (`HNG-EXH-006`)*: in `DamageSet::PacketReport` and `DamageSet::Report`, reads
     `Damaged` for player targets with `report.health > 0.0`, looking up the registry
     `DamageType::exhaustion` value.
   - *`minecraft:hunger` effect (`HNG-EXH-007`)*: adds `0.005 * (amplifier + 1)` per active effect
     tick inside the hunger tick.
2. **Tick step in `DamageSet::Collect`**:
   - Runs in `TickSet::WorldWrite` after `MotionPass` and before `DamageSet::Filter` /
     `DamageSet::Apply`. Reads `PlayerFileState`, `Vitals`, `AttributeInstances` (`max_health`),
     `PlayerGameMode`, `GameRules` (`GameRule::NaturalHealthRegeneration`), `ServerDifficulty`
     (a `Resource` wrapper around `mcrs_minecraft_save::Difficulty`) and the new player lived tick
     count (skipping spectators touching unloaded chunks via the new `HNG-TICK-001` check).
   - Calls `HungerState::tick` and applies `set_if_neq` on `PlayerFileState` and `Vitals`
     (`send_health`):
     - Updates `PlayerFileState` with `set_if_neq` only when the four hunger fields change, so idle
       ticks never mark `PlayerFileState` changed.
     - On `next_health`, updates `Vitals` (`vitals.set_if_neq`) (`HNG-REG-003`: no `PendingHits`, no
       `Damaged`, no `damage_event`, no hurt sound/animation, `absorption` unchanged).
     - On `starve`, pushes `DamageRequest::new(player, Hit::new(1.0, DamageSource::of(starve_id)), ...)`
       into `PendingHits` so `DamageSet::Filter` and `DamageSet::Apply` (`apply_hits`) enforce
       invulnerability, absorption, cooldown, death and `damage_event` (`HNG-STV-001`, `HNG-STV-002`).
3. **`send_health` and wire sync (`TickSet::Project`)**:
   - `send_health` (`mcrs_minecraft_engine_player_vitals`) runs `.in_set(TickSet::Project).after(DamageSet::Report)`.
   - Because both `PlayerFileState` hunger updates and `Vitals` starvation/heal updates finish before
     `TickSet::Project`, `send_health` observes `Or<(Changed<Vitals>, Changed<PlayerFileState>)>` once
     per tick, compares `SentHealth { health, food, saturation: Empty | Positive }` using
     `set_if_neq` (`HNG-NET-001`, `HNG-NET-002`), and emits at most one `ClientboundSetHealth`
     (`set_health`, play 106) per tick (`HNG-TICK-002`).
   - On the client (`mcrs_minecraft_client_world`), `on_set_health` syncs `Vitals` and `FoodLevel`
     via `set_if_neq`; `respawn_players` resets `Vitals`, resets `PlayerFileState` hunger fields and
     removes `SentHealth` so the next tick resends `set_health`.

## 5. Prerequisite engine touchpoints

- **World `Difficulty` resource**: `mcrs_minecraft_save::Difficulty` has no `Resource` derive
  (`mcrs_minecraft_save` has no `bevy_ecs` dependency); define a Layer-4 `ServerDifficulty`
  `Resource` wrapper (`mcrs_minecraft_engine_world`) and expose it from `LevelDat` through
  `DimInput` into dimension worlds alongside `GameRules`.
- **Player lived tick count (`t` in `HNG-REG-004`)**: add a Layer-4 per-player tick counter
  component (distinct from world `GameTime`) initialized to `0` on join/respawn and incremented once
  per server tick.
- **Spectator unloaded-chunk check (`HNG-TICK-001`)**: add a Layer-4 check in
  `mcrs_minecraft_engine_world` (over `ColumnIndex` / `SectionStage`) testing whether a
  spectator's bounding box intersects an unloaded chunk so `VanillaHungerPlugin` skips its hunger
  tick.
- **Respawn timer reset**: extend `reset_file_state` in
  `crates/mcrs_minecraft_engine_player_vitals/src/respawn_players.rs` to reset `food_tick_timer`
  to `0` (`HNG-STA-001`).
- **Damage exhaustion lookup & `starve` id**: bake per-type `exhaustion` values and cache the
  `minecraft:starve` id alongside `DamageTypeRules` (`mcrs_minecraft_damage`) in
  `bake_damage_types` (`mcrs_minecraft_engine_damage`).
- **Accepted player move delta**: expose per-tick accepted displacement and prior `ClientOnGround`
  from `process_movement` (`TickSet::ClientInput`) so teleports and rejected moves add no exhaustion.
