# Weather, rain, thunder, and lightning: behavioural specification

Version: see `assets/minecraft/version.json`. Wire packet IDs: `assets/mcrs/reports/packets.json`.
Registry IDs: `assets/mcrs/reports/registries.json`.

This document states the global weather cycle, strength ramp, wire sync, lightning strikes, and
world save format. Command syntax and messages are in `command-weather.md`. Sky and fog layers are
in `environment-attributes.md` (`ENVA-LAYER-005`). This document does not prescribe an
architecture.

## 0. Conventions

### 0.1 Terms

- **Weather state**: the five global world values: **clear time**, **rain time**, **thunder time**,
  **rain flag**, and **thunder flag**.
- **Clear time**: the non-negative tick count (`clear_weather_time`) during which clear weather
  holds before natural rain or thunder can start.
- **Rain time**: the non-negative tick count (`rain_time`) until **rain flag** toggles when **clear
  time** is `0`.
- **Thunder time**: the non-negative tick count (`thunder_time`) until **thunder flag** toggles
  when **clear time** is `0`.
- **Rain flag**: the boolean (`raining`) that sets target **rain strength** to `1.0` when true or
  `0.0` when false.
- **Thunder flag**: the boolean (`thundering`) that sets target **thunder strength** to `1.0` when
  true or `0.0` when false.
- **Weather dimension**: a dimension whose type has `has_skylight` true, `has_ceiling` false, and
  key other than `minecraft:the_end` (`environment-attributes.md`, `ENVA-LAYER-005`).
- **Rain strength**: a `binary32` value in `[0.0, 1.0]` on a **weather dimension**.
- **Thunder strength**: a `binary32` value in `[0.0, 1.0]` on a **weather dimension**.
- **Wet state**: the boolean `rain_strength > 0.2` (`binary32`) of a **weather dimension**.
- **Thunderstorm**: the state where both **wet state** holds and **thunder strength** is greater
  than `0.2` (`binary32`).
- **Lightning strike**: the spawn of a `minecraft:lightning_bolt` (`80`) entity during a
  **thunderstorm**.

### 0.2 Provenance and kinds

Labels follow `registry.md`, section 0.3 (`black_box_confirmed`, `documentation`, `design`,
`unresolved`). Kinds follow `docs/spec/README.md`, section "Semantics, not accidents".

### 0.3 ID prefixes

| Prefix | Subject |
|--------|---------|
| `WTH-CYC` | global weather state and natural cycle timers |
| `WTH-LVL` | per-dimension strength ramp and `advance_weather` |
| `WTH-NET` | clientbound `game_event` weather sync packets |
| `WTH-LGT` | lightning strike conditions, targets, and effects |
| `WTH-SAV` | persistence of `data/minecraft/weather.dat` |
| `WTH-E` | verification experiments |

### 0.4 Artefacts cited

- `assets/mcrs/reports/packets.json` (`game_event` `39`, `add_entity` `1`, `sound` `119`)
- `assets/mcrs/reports/registries.json` (`minecraft:advance_weather` `1`,
  `minecraft:lightning_bolt` `80`, `minecraft:entity.lightning_bolt.impact` `662`,
  `minecraft:entity.lightning_bolt.thunder` `663`)
- `assets/mcrs/registry_definition/game_rule.json` (`minecraft:advance_weather` default `true`)
- `tools/mcrs_minecraft_probe/scenarios/weather.json`
- `tools/mcrs_minecraft_probe/traces/weather.json`

---

## 1. Global weather state and natural cycle

### WTH-CYC-001: Global weather state and weather dimensions

The server must hold one **weather state** across the whole world. Only a **weather dimension**
tracks **rain strength**, **thunder strength**, weather packets, and **lightning strikes**. Any
other dimension must keep **rain strength** `0.0` and **thunder strength** `0.0` and must send no
weather `game_event` packet.

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`:
`join_alice_clear`, `weather_rain`, `join_bob_during_rain`) for overworld packet delivery;
`documentation` (and `environment-attributes.md`, `ENVA-LAYER-005`) for global shared state and the
**weather dimension** condition (`has_skylight`, no ceiling, not `minecraft:the_end`). Kind:
`semantic`. Confidence: `high`.

### WTH-CYC-002: Clear timer override

In each normal server tick when `minecraft:advance_weather` (`1`) is true and **clear time** is
greater than `0`:

1. Decrease **clear time** by `1`.
2. Set **rain time** to `0` and **thunder time** to `0`.
3. Set **rain flag** to false and **thunder flag** to false.

While **clear time** stays above `0`, natural rain and thunder must not start.

Provenance: `black_box_confirmed` (`assets/mcrs/reports/registries.json`,
`assets/mcrs/registry_definition/game_rule.json`) for rule id and default; `documentation` for
timer reset and forced clear. Kind: `semantic`. Confidence: `high`.

### WTH-CYC-003: Natural rain and thunder timers

When `minecraft:advance_weather` is true and **clear time** is `0`, each normal server tick must
advance **thunder time** and **rain time**:

1. If **thunder time** is `0` or less, toggle **thunder flag**.
2. On that toggle, draw **thunder time** uniformly from `[3600, 15600]` when true, or `[12000,
   180000]` when false.
3. Else decrease **thunder time** by `1`.
4. If **rain time** is `0` or less, toggle **rain flag**.
5. On that toggle, draw **rain time** uniformly from `[12000, 24000]` when true, or `[12000,
   180000]` when false.
6. Else decrease **rain time** by `1`.

When `minecraft:advance_weather` is false, all five fields of **weather state** must stay frozen
until a command changes them.

Provenance: `documentation` (`command-gamerule.md`, `CMD-GR-004`) for natural cycle timer
progression, uniform duration bounds, and timer freeze under `advance_weather` false. Kind:
`semantic`. Confidence: `high`.

---

## 2. Strength ramp and `advance_weather`

### WTH-LVL-001: Per-tick `0.01` strength ramp

In each normal tick of a **weather dimension**, the server must update both strengths in `binary32`
arithmetic:

1. Add `0.01` to **rain strength** when **rain flag** is true, or subtract `0.01` when false.
2. Clamp **rain strength** to `[0.0, 1.0]`.
3. Add `0.01` to **thunder strength** when **thunder flag** is true, or subtract `0.01` when false.
4. Clamp **thunder strength** to `[0.0, 1.0]`.

In `binary32`, `100` steps of `±0.01` reach `0.99999934` upward or `6.7055225e-7` downward. Thus a
full transition takes `101` ticks.

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`: `weather_rain`,
`weather_thunder`, `weather_clear_and_stop_advance`, `set_thunder_for_save`). Kind: `semantic`,
`wire`. Confidence: `high`.

### WTH-LVL-002: Strength ramp under `advance_weather` false

The strength ramp in `WTH-LVL-001` must run even when `minecraft:advance_weather` is false. After
`gamerule advance_weather false` and `weather clear`, both strengths still ramp to `0.0` at `0.01`
per tick. All five fields of **weather state** stay frozen (`WTH-CYC-003`).

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`:
`weather_clear_and_stop_advance`) for the strength ramp to `0.0` while `advance_weather` is
false; `documentation` for the freeze of all five fields of **weather state** (`WTH-CYC-003`). Kind:
`semantic`. Confidence: `high`.

### WTH-LVL-003: Wet threshold and world start state

A **weather dimension** enters **wet state** when **rain strength** exceeds `0.2` (`binary32`). On
the upward ramp from `0.0`, **rain strength** crosses `0.2` on tick `20` (`0.20000002`). On the
downward ramp from `1.0`, **rain strength** reaches `0.19000068` (`<= 0.2`) on tick `81`.

When a world loads from save, each **weather dimension** sets **rain strength** to `1.0` if
**rain flag** is true, else `0.0`. It also sets **thunder strength** to `1.0` if **thunder flag** is
true, else `0.0`. No startup ramp occurs.

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`: `weather_rain`
event `[20]`, `weather_clear_and_stop_advance` event `[162]`, `set_thunder_for_save` event `[40]`,
`join_alice_after_restart`). Kind: `semantic`, `wire`. Confidence: `high`.

---

## 3. Clientbound wire sync

### WTH-NET-001: Weather `game_event` (`39`) layout

The `game_event` packet (`play`, clientbound ID `39`) sends `event: i8` followed by
`param: binary32`. Weather sync uses these event IDs:

| Event ID | Name | Parameter (`binary32`) |
|----------|------|------------------------|
| `1` | `begin_raining` | `0.0` |
| `2` | `end_raining` | `0.0` |
| `7` | `rain_level_change` | current **rain strength** in `[0.0, 1.0]` |
| `8` | `thunder_level_change` | current **thunder strength** in `[0.0, 1.0]` |

Provenance: `black_box_confirmed` (`assets/mcrs/reports/packets.json`;
`tools/mcrs_minecraft_probe/traces/weather.json`: `weather_rain`, `join_bob_during_rain`,
`weather_thunder`, `weather_clear_and_stop_advance`, `join_alice_after_restart`). Kind: `wire`.
Confidence: `high`.

### WTH-NET-002: Per-tick change packets and threshold burst

In each tick of a **weather dimension**, the server inspects changes from the previous tick. It
sends to all players in that dimension:

1. If **rain strength** changed, send `7` (`rain_level_change`) with the new **rain strength**.
2. If **thunder strength** changed, send `8` (`thunder_level_change`) with the new **thunder
   strength**.
3. If **wet state** changed to true, send `1` (`begin_raining`, `0.0`). If it changed to false,
   send `2` (`end_raining`, `0.0`).
4. Right after step 3, send `7` (`rain_level_change`) and `8` (`thunder_level_change`) with current
   strengths.

When a strength stays equal to the previous tick, the server must send no packet for it. Every
player in that dimension receives identical payload bytes for one broadcast event.

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`: `weather_rain`,
`weather_thunder`, `weather_clear_and_stop_advance`, `set_thunder_for_save`) for packet selection
and order; `design` for identical shared payload bytes across recipients. Kind: `wire`, `semantic`.
Confidence: `high`.

### WTH-NET-003: Join and dimension-entry sync

A player can join a **weather dimension** or enter one on dimension change (`dimension-change.md`,
`DIM-CHG-003` step 11).

- If **wet state** holds (`rain_strength > 0.2`), send in order before `13` (`level_chunks_load_start`,
  `0.0`):
  1. `1` (`begin_raining`, `0.0`);
  2. `7` (`rain_level_change`, current **rain strength**);
  3. `8` (`thunder_level_change`, current **thunder strength**).
- If **wet state** does not hold (`rain_strength <= 0.2`), send no weather `game_event` packet
  before `13` (`level_chunks_load_start`).

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`:
`join_alice_clear`, `join_bob_during_rain`, `join_alice_after_restart`). Kind: `wire`. Confidence:
`high`.

---

## 4. Lightning strikes

### WTH-LGT-001: Strike chance and thunderstorm gate

Lightning runs in each normal tick of a **weather dimension** (`block-simulation.md`, `SIM-TICK-002`
step 9.3). When **thunderstorm** holds, each chunk in the active area rolls a `1` in `100000`
chance per tick. A successful roll attempts one **lightning strike**.

Provenance: `documentation` for the `1 / 100000` chance and storm gate; `black_box_confirmed`
(`assets/mcrs/reports/registries.json`) for `minecraft:lightning_bolt` (`80`). Kind: `semantic`.
Confidence: `high`.

### WTH-LGT-002: Column target, lightning rod redirect, and rain test

When a **lightning strike** triggers in a chunk:

1. Select a random `(x, z)` column in that chunk.
2. Find the position above the highest liquid block or block with property "blocks motion".
3. Search for a `minecraft:lightning_rod` within a spherical radius of `128` blocks.
4. If found, redirect the target to the top of that rod.
5. Cancel the strike unless rain falls at the final target (`environment-damage.md`, `ENV-FIR-007`),
   with no snow or dry biome.

Provenance: `documentation`. Kind: `semantic`. Confidence: `high`.

### WTH-LGT-003: Spawn, sounds, fire, and damage

When `WTH-LGT-002` passes, the server performs these effects:

1. Spawn a `minecraft:lightning_bolt` (`80`) entity with `add_entity` (`1`).
2. Emit `minecraft:entity.lightning_bolt.thunder` (`663`) up to `160000` blocks and
   `minecraft:entity.lightning_bolt.impact` (`662`) via `sound` (`119`).
3. Ignite fire on exposed impact blocks, except on a lightning rod.
4. Apply `5` base `minecraft:lightning_bolt` damage (`entity-damage.md`) to struck entities.

Provenance: `documentation` for fire, `5` base damage, and thunder distance; `black_box_confirmed`
(`assets/mcrs/reports/packets.json`, `assets/mcrs/reports/registries.json`) for `add_entity` (`1`),
`sound` (`119`), entity id `80`, and sound ids `662` and `663`. Kind: `semantic`, `wire`.
Confidence: `high`.

---

## 5. Persistence

### WTH-SAV-001: `data/minecraft/weather.dat`

The server saves **weather state** to `<save>/data/minecraft/weather.dat` under `DataVersion` and
`data` (`persistence.md`, `SAVE-DATA-001`). All five fields are required: `clear_weather_time` (int),
`rain_time` (int), `thunder_time` (int), `raining` (byte), and `thundering` (byte).

On load, negative counters clamp to `0`, and an absent file starts clear. An unreadable file stays
untouched while the world starts clear. After `save-all flush` during a thunderstorm and restart,
**rain flag** and **thunder flag** restore at strength `1.0` (`WTH-LVL-003`, `WTH-NET-003`).

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`:
`set_thunder_for_save`, `restart_world`, `join_alice_after_restart`) for cross-restart storm sync;
`documentation` (`persistence.md`, `SAVE-DATA-001`) for the five required fields. Kind: `semantic`,
`wire`. Confidence: `high`.

---

## 6. Experiments

- **WTH-E1**: `tools/mcrs_minecraft_probe/scenarios/weather.json` and
  `tools/mcrs_minecraft_probe/traces/weather.json`.
  1. Run `prepare` (`advance_weather true`, `weather clear 600`), then join `Alice`
     (`join_alice_clear`): `Alice` receives only `level_chunks_load_start`.
  2. Run `weather rain 400` (`weather_rain`): `Alice` receives `101` `rain_level_change` steps from
     `0.01` to `1.0` plus the tick-`20` burst `[begin_raining(0.0), rain_level_change(0.20000002),
     thunder_level_change(0.0)]` (`104` events total).
  3. Join `Bob` (`join_bob_during_rain`): `Bob` receives `begin_raining(0.0)`,
     `rain_level_change(1.0)`, `thunder_level_change(0.0)`, and `level_chunks_load_start(0.0)`.
  4. Run `weather thunder 400` (`weather_thunder`): `Alice` and `Bob` each receive `101`
     `thunder_level_change` steps from `0.01` to `1.0` and zero `rain_level_change`.
  5. Run `gamerule advance_weather false` and `weather clear 200` (`weather_clear_and_stop_advance`):
     both players receive `101` downward steps to `0.0` for rain and thunder plus the tick-`81`
     burst `[end_raining(0.0), rain_level_change(0.19000068), thunder_level_change(0.19000068)]`.
  6. Run `gamerule advance_weather true`, `weather thunder 400`, and `save-all flush`
     (`set_thunder_for_save`), restart (`restart_world`), and join `Alice` (`join_alice_after_restart`):
     `Alice` receives `begin_raining(0.0)`, `rain_level_change(1.0)`, `thunder_level_change(1.0)`,
     and `level_chunks_load_start(0.0)`.

---

## 7. Unresolved

- Probe captures for omitted `<duration>` on `/weather clear`, `/weather rain`, and
  `/weather thunder`, plus `execute store result` return values (`-1` vs `d`).
- Probe captures beyond timer expiry (and `weather.dat` inspection) for natural rollover
  (`clear -> rain -> clear`, thunder overlap) and timer freeze under `advance_weather` false.
- Non-weather dimension probes (`minecraft:the_nether`, `minecraft:the_end`, `has_ceiling` types).
- Probe capture for `lightning_bolt` spawns, `lightning_rod` interception, skeleton trap horses, and
  struck-entity damage and fire blocks (`WTH-LGT-001`..`WTH-LGT-003`).
