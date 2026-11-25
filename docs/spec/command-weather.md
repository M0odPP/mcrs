# The `weather` command: behavioural specification

| Field | Value |
|-------|-------|
| Parser ids | `minecraft:time` 43 |

Version: see `assets/minecraft/version.json`. Source of the parser ids: `assets/mcrs/reports/registries.json`, registry
`minecraft:command_argument_type`.

This document states the tree, subcommands, weather state updates, and tick rules for `/weather`. It
covers `clear`, `rain`, and `thunder`. `weather.md` states the full weather cycle, strength ramp,
lightning, and save rules. This document does not prescribe an architecture.

## 0. Conventions

- **Node**, **source**, **error**: as `command-parser.md` defines them.
- **Broadcast flag**, **system message**: as `commands-packet.md` defines them.
- **Result**: as `command-execute.md`, `CMD-EX-040`, defines it.
- **Weather state**: **clear time**, **rain time**, **thunder time**, **rain flag**, and **thunder
  flag** (`weather.md`, section 0.1).
- **Rain strength**, **thunder strength**, **weather dimension**: as `weather.md`, section 0.1,
  defines them.
- Provenance labels follow `registry.md`, section 0.3 (`black_box_confirmed`, `documentation`).
  Kinds follow `docs/spec/README.md`, section "Semantics, not accidents". ID prefix: `CMD-W`.

---

### CMD-W-001: Tree

```
weather                                        require command_level gamemasters
├── clear                                      executable
│   └── <duration: minecraft:time>             executable; min 1
├── rain                                       executable
│   └── <duration: minecraft:time>             executable; min 1
└── thunder                                    executable
    └── <duration: minecraft:time>             executable; min 1
```

The permission requirement is `command_level` `gamemasters` (`command-permissions.md`,
`PERM-LEVEL-001`). `command-time.md`, `CMD-TIME-003`, states `minecraft:time`. The `<duration>`
node sets integer minimum `1` tick.

Provenance: `black_box_confirmed` (`assets/mcrs/reports/registries.json`;
`tools/mcrs_minecraft_probe/traces/weather.json`: `prepare`, `weather_rain`, `weather_thunder`,
`weather_clear_and_stop_advance`) for parser id `43` and all three subcommands; `documentation` for
the `gamemasters` requirement and minimum `1` tick. Kind: `wire`, `client agreement`. Confidence:
`high`.

---

### CMD-W-002: `weather clear`

- **Inputs:** optional `<duration>` in ticks (`d`).
- **Behavior:**
  1. If `<duration>` exists, set `d` to `<duration>`.
  2. Else draw `d` uniformly from `12000` to `180000`, inclusive.
  3. Set **clear time** to `d`, **rain time** to `0`, and **thunder time** to `0`.
  4. Set **rain flag** to false and **thunder flag** to false.
  5. Send `commands.weather.set.clear`, with no argument, as a success message with the
     **broadcast flag** set.
- **Postconditions:**
  - When `<duration>` exists, **result** is `d`. Else **result** is `-1`.
  - **Rain strength** and **thunder strength** ramp down under `CMD-W-003` (`WTH-LVL-001`).

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`: `prepare`,
`weather_clear_and_stop_advance`) for explicit duration, `commands.weather.set.clear`, operator
broadcast, and post-clear ramp; `documentation` for the `[12000, 180000]` draw, timer writes, and
`-1` result. Kind: `semantic`, `wire`. Confidence: `high`.

---

### CMD-W-004: `weather rain`

- **Inputs:** optional `<duration>` in ticks (`d`).
- **Behavior:**
  1. If `<duration>` exists, set `d` to `<duration>`.
  2. Else draw `d` uniformly from `12000` to `24000`, inclusive.
  3. Set **clear time** to `0`, **rain time** to `d`, and **thunder time** to `d`.
  4. Set **rain flag** to true and **thunder flag** to false.
  5. Send `commands.weather.set.rain`, with no argument, as a success message with the
     **broadcast flag** set.
- **Postconditions:**
  - When `<duration>` exists, **result** is `d`. Else **result** is `-1`.
  - **Rain strength** ramps toward `1.0` and **thunder strength** ramps toward `0.0` (`WTH-LVL-001`).

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`: `weather_rain`)
for explicit duration, `commands.weather.set.rain`, operator broadcast, and strength ramp;
`documentation` for the `[12000, 24000]` draw, timer writes, and `-1` result. Kind: `semantic`,
`wire`. Confidence: `high`.

---

### CMD-W-005: `weather thunder`

- **Inputs:** optional `<duration>` in ticks (`d`).
- **Behavior:**
  1. If `<duration>` exists, set `d` to `<duration>`.
  2. Else draw `d` uniformly from `3600` to `15600`, inclusive.
  3. Set **clear time** to `0`, **rain time** to `d`, and **thunder time** to `d`.
  4. Set **rain flag** to true and **thunder flag** to true.
  5. Send `commands.weather.set.thunder`, with no argument, as a success message with the
     **broadcast flag** set.
- **Postconditions:**
  - When `<duration>` exists, **result** is `d`. Else **result** is `-1`.
  - Both **rain strength** and **thunder strength** ramp toward `1.0` (`WTH-LVL-001`).

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`:
`weather_thunder`, `set_thunder_for_save`) for explicit duration, `commands.weather.set.thunder`,
operator broadcast, and strength ramp; `documentation` for the `[3600, 15600]` draw, timer writes,
and `-1` result. Kind: `semantic`, `wire`. Confidence: `high`.

---

### CMD-W-003: The weather in each tick

- **Behavior:** in each normal tick of a **weather dimension**:
  1. If `minecraft:advance_weather` is true, advance the global **weather state** timers (`WTH-CYC-002`,
     `WTH-CYC-003`).
  2. If `minecraft:advance_weather` is false, freeze all five fields of **weather state** (`WTH-CYC-003`).
  3. Move **rain strength** by `0.01` toward `1.0` when **rain flag** is true, or toward `0.0` when
     false (`WTH-LVL-001`).
  4. Move **thunder strength** by `0.01` toward `1.0` when **thunder flag** is true, or toward `0.0`
     when false (`WTH-LVL-001`).
  5. Clamp both strengths to `[0.0, 1.0]`.
- **Postconditions:**
  - Steps 3 to 5 run also when `minecraft:advance_weather` is false (`WTH-LVL-002`).
  - Each strength change sends `game_event` (`39`) packets to players of that dimension
    (`WTH-NET-002`).

Provenance: `black_box_confirmed` (`tools/mcrs_minecraft_probe/traces/weather.json`: `weather_rain`,
`weather_thunder`, `weather_clear_and_stop_advance`, `set_thunder_for_save`) for the `0.01` per-tick
strength ramp (with `advance_weather` true or false) and `game_event` (`39`) delivery;
`documentation` for the timer advance and freeze rules of global **weather state**. Kind: `semantic`,
`wire`. Confidence: `high`.

---

## Experiments

- `tools/mcrs_minecraft_probe/scenarios/weather.json` and
  `tools/mcrs_minecraft_probe/traces/weather.json` (`WTH-E1`):
  - `prepare` sets `advance_weather` to `true` and runs `weather clear 600`.
  - `weather_rain` runs `weather rain 400`; `Alice` receives `chat.type.admin` with
    `commands.weather.set.rain`.
  - `weather_thunder` runs `weather thunder 400`; `Alice` and `Bob` receive `chat.type.admin` with
    `commands.weather.set.thunder`.
  - `weather_clear_and_stop_advance` sets `advance_weather` to `false` and runs `weather clear 200`;
    operators receive `commands.weather.set.clear`, and strengths still ramp to `0.0`.
