# Weather cycle, strength ramp, and lightning: design

Design note for server-side global weather state, per-dimension rain and thunder ramps, network sync,
`/weather clear|rain|thunder [duration]`, and thunderstorm lightning strikes (`#740`). Behaviour comes
from `docs/spec/weather.md` (`WTH`) and `docs/spec/command-weather.md` (`CMD-W`).

## 1. Classification of the state

| Value | Kind | Owner |
|-------|------|-------|
| `Weather` (`clear_ticks`, `rain_ticks`, `thunder_ticks`, `raining`, `thundering`) on host | authoritative truth | `ServerStateHostPlugin` and `WorldStatePlugin` in `mcrs_minecraft_engine` (passive storage + `ServerEdit` writes); vanilla cycle progression in Layer-5 `VanillaWeatherHostPlugin` |
| `WeatherData` (`data/minecraft/weather.dat`) | persisted snapshot | `mcrs_minecraft_save` read and written by `WorldStatePlugin` |
| `Weather` replicated into each dimension `SubApp` | read-only host snapshot (`DimInput`) | synced on `changed::<Weather>` by `DimCells` in `mcrs_minecraft_engine` |
| `WeatherLevel` (`mcrs_minecraft_dimension_environment::environment::WeatherLevel { rain, thunder }`) on each `HasWeather` dimension | authoritative per-dimension level truth (`wet` derived as `rain > 0.2`) | `VanillaWeatherPlugin` in `mcrs_minecraft_vanilla_weather` |
| `DimensionWeatherLevels` host mirror (`Entity -> WeatherLevel`) | read-only host mirror for join & departure routing | updated from `DimOutput::weather_level` in `take_dim_outputs` before `route_departure` / `DimDelivered` |
| `ServerEdit::{ClearWeather, RainWeather, ThunderWeather}` | temporal command edit | queued by `mcrs_minecraft_command_world`, routed via `DimMail::edit_server` to `apply_edit` |
| `LightningStrike` / `ClientboundAddEntity(LightningBolt)` | temporal world effect | emitted by lightning pass in `VanillaWeatherPlugin` |
| sky tint, fog density, cloud color (`WeatherLevel` on client) | derived on client | `mcrs_minecraft_client` (`ENVA-LAYER-005`) |

## 2. Crate map

| Crate | Layer | Content |
|-------|-------|---------|
| `mcrs_minecraft_weather` (`#![no_std]`, new) | 3, rules | `WeatherState`, `WeatherCycleRng`, `WeatherCommand`, `step_level(level: f32, target: bool) -> f32`, `is_wet(rain: f32) -> bool` (`rain > 0.2`), and cycle timer rules `WTH-CYC-*`, `WTH-LVL-*` (reuses `WeatherLevel` instead of defining a second struct) |
| `mcrs_minecraft_dimension_environment` | 3, data | Canonical `WeatherLevel { rain: f32, thunder: f32 }` shared by client sky and server dimensions |
| `mcrs_minecraft_save` | 3, data | `WeatherData`, `read_weather`, `write_weather` (`WTH-SAV-001`) |
| `mcrs_minecraft_protocol_game` | 3, data | `ClientboundGameEvent`, `GameEventKind::{BeginRaining, EndRaining, RainLevelChange, ThunderLevelChange}`, `ClientboundAddEntity`, `ClientboundSound` (new) (`WTH-NET-001`) |
| `mcrs_minecraft_engine_world` | 4 | `HasWeather` marker component on weather-capable dimensions |
| `mcrs_minecraft_engine` | 4 | `ServerEdit::{ClearWeather, RainWeather, ThunderWeather}`, passive `Weather` resource, `WorldStatePlugin` load/autosave, `DimCells`/`DimInput` sync, `DimensionWeatherLevels` host mirror + `DimOutput::weather_level` (new), `enter_level_packets` weather arg, `PacketTarget::AllInDim`, `PacketPayload::{AddEntity, Sound}` (new) |
| `mcrs_minecraft_command_world` | 5 | `/weather clear|rain|thunder [<duration>]` emitting `ServerEdit` and `commands.weather.set.{clear,rain,thunder}` (`CMD-W-001`..`005`) |
| `mcrs_minecraft_vanilla_weather` (new) | 5 | `VanillaWeatherHostPlugin` (host-side vanilla cycle timers `WTH-CYC-002`..`003`) and `VanillaWeatherPlugin` on `HasWeather` dimensions (startup `WeatherLevel` init, per-tick `0.01f32` ramp, broadcast-once `Bytes` `game_event` fanout, lightning strike pass) |
| `mcrs_minecraft_server` | 7 | Adds `VanillaWeatherHostPlugin` and dimension `VanillaWeatherPlugin` (`AddDimensionPlugins`) in `MinecraftServerPlugins` |

Reason for `#![no_std]` `mcrs_minecraft_weather`: client prediction, bots, replay tools, and
dimension sub-apps share the `f32` `0.01`/tick ramp arithmetic, `is_wet` threshold predicate, and
cycle timer rules without pulling in Bevy, disk I/O, or engine registries, while storing the single
canonical `WeatherLevel` from `mcrs_minecraft_dimension_environment`. Adding the two new crates in
the implementation PR requires a published RFD (`CONTRIBUTING.md`).

## 3. Host-dimension sync and Layer-4 neutrality

1. **Layer 4 stays free of vanilla cycle behaviour (`AGENTS.md`)**: `ServerStateHostPlugin` and
   `WorldStatePlugin` in `mcrs_minecraft_engine` only load/save `data/minecraft/weather.dat` into
   `Weather` (`max(0)` clamp on loaded timers) and apply explicit `ServerEdit::{ClearWeather,
   RainWeather, ThunderWeather}` writes. Without `VanillaWeatherHostPlugin`, `Weather::default()`
   zeros never auto-start random rain or thunder.
2. **Layer-5 vanilla cycle progression**: `VanillaWeatherHostPlugin` advances host `Weather` timers
   in `FixedUpdate` (gated on `tick_runs_normally` and `GameRule::AdvanceWeather`) using
   `mcrs_minecraft_weather` (`WTH-CYC-002`, `WTH-CYC-003`). When `advance_weather` is false, all five
   fields stay frozen.
3. **Host-to-dimension and command routing**: `DimCells` forwards `changed::<Weather>` alongside
   `GameRules` into each dimension `SubApp` via `DimInput`. `/weather clear|rain|thunder [<duration>]`
   in `mcrs_minecraft_command_world` emits `ServerChange(ServerEdit::*)`, routed through
   `DimMail::edit_server` -> `DimOutput::server_edits` -> `apply_host_edit` on the host, returning
   `<duration>` or `-1` (`CMD-W-002`..`005`).

## 4. Per-dimension tick, join/departure sync, and lightning

1. **Startup initialization (`WTH-LVL-003`)**: on the first tick after world load in a `HasWeather`
   dimension, `VanillaWeatherPlugin` inserts/initializes canonical `WeatherLevel` directly from host
   `Weather` (`1.0` if true, `0.0` if false), avoiding a startup ramp across restarts
   (`join_alice_after_restart`), and initializes the host `DimensionWeatherLevels` entry via
   `DimensionBuilders::add_host`.
2. **Per-tick ramp and `Bytes` broadcast (`WTH-LVL-001`, `WTH-LVL-002`, `WTH-NET-002`)**: in
   `TickSet::Project` (running on `HasWeather` dimensions whenever `tick_runs_normally`, even when
   `advance_weather` is false), `VanillaWeatherPlugin` steps `WeatherLevel` via
   `mcrs_minecraft_weather::step_level` toward `Weather.raining` and `Weather.thundering`. On any
   change (`rain`, `thunder`, or `is_wet(rain)` transition), it broadcasts `ClientboundGameEvent`
   through `PacketTarget::AllInDim` (`bridge_outbound::encode_once` encodes one shared `Bytes`
   buffer for all recipients) and reports updated `WeatherLevel` in `DimOutput::weather_level`.
3. **Join and dimension-entry sync BEFORE `LevelChunksLoadStart` (`WTH-NET-003`)**: in
   `crates/mcrs_minecraft_engine/src/muxer/departure_routing.rs`, `route_departure` calls `announce`
   on the **host** (emitting `Respawn` and `enter_level_packets` including `LevelChunksLoadStart`)
   BEFORE sending `PlayerTransfer::Arrive` to the destination `SubApp`. Because `take_dim_outputs`
   updates the host `DimensionWeatherLevels` mirror from `DimOutput::weather_level` before
   `route_departure`, `announce` reads `DimensionWeatherLevels.get(destination.label)` and passes
   `Option<WeatherLevel>` into `enter_level_packets` (omitted when absent or `!is_wet(rain)`). On
   initial join, `consume_inbound_player_spawn` inside the `SubApp` passes the dimension's current
   `Option<WeatherLevel>` into the same `enter_level_packets` helper. Both paths therefore emit
   `[BeginRaining(0.0), RainLevelChange(rain), ThunderLevelChange(thunder)]` right BEFORE
   `LevelChunksLoadStart`.
4. **Lightning pass (`WTH-LGT-001`..`003`)**: in `TickSet::WorldWrite` on `HasWeather` dimensions
   during active thunder (`is_wet(rain) && thunder > 0.2`), each active ticked column chunk rolls
   `1 / 100_000`, picks a sky-exposed heightmap column, redirects to any `minecraft:lightning_rod`
   within `128` blocks, verifies rain precipitation at the target, and emits
   `PacketPayload::AddEntity` (`minecraft:lightning_bolt`) and `PacketPayload::Sound`
   (`entity.lightning_bolt.{thunder,impact}`), fire ignition, and `5` base damage.
