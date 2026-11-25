# Weather cycle, strength ramp, and lightning: design

Design note for server-side global weather state, per-dimension rain and thunder ramps, network sync,
`/weather clear|rain|thunder [duration]`, and thunderstorm lightning strikes (`#740`). Behaviour comes
from `docs/spec/weather.md` (`WTH`) and `docs/spec/command-weather.md` (`CMD-W`).

## 1. Classification of the state

| Value | Kind | Owner |
|-------|------|-------|
| `Weather` (`WeatherState`: `clear_ticks`, `rain_ticks`, `thunder_ticks`, `raining`, `thundering`) on host | authoritative truth | `ServerStateHostPlugin` and `WorldStatePlugin` in `mcrs_minecraft_engine` |
| `WeatherData` (`data/minecraft/weather.dat`) | persisted snapshot | `mcrs_minecraft_save` read/written by `WorldStatePlugin` |
| `Weather` replicated into each dimension `SubApp` | read-only host snapshot (`DimInput`) | synced on `changed::<Weather>` by `DimCells` in `mcrs_minecraft_engine` |
| `WeatherLevel` (`rain`, `thunder`, `wet`) on each `HasWeather` dimension | authoritative per-dimension level truth | `VanillaWeatherPlugin` in `mcrs_minecraft_vanilla_weather` |
| `ServerEdit::{ClearWeather, RainWeather, ThunderWeather}` | temporal command edit | queued by `mcrs_minecraft_command_world`, routed via `DimMail::edit_server` to `apply_edit` |
| `LightningStrike` / `ClientboundAddEntity(LightningBolt)` | temporal world effect | emitted by lightning pass in `VanillaWeatherPlugin` |
| sky tint, fog density, cloud color (`WeatherLevel` on client) | derived on client | `mcrs_minecraft_client` (`ENVA-LAYER-005`) |

## 2. Crate map

| Crate | Layer | Content |
|-------|-------|---------|
| `mcrs_minecraft_weather` (`#![no_std]`) | 3, rules | `WeatherState`, `WeatherCycleRng`, `WeatherCommand`, `WeatherLevel`, `WeatherStep` (`0.01f32` ramp, `wet > 0.2`, cycle timer rules `WTH-CYC-*`, `WTH-LVL-*`) |
| `mcrs_minecraft_save` | 3, data | `WeatherData`, `read_weather`, `write_weather` (`WTH-SAV-001`) |
| `mcrs_minecraft_protocol_game` | 3, data | `ClientboundGameEvent`, `GameEventKind::{BeginRaining, EndRaining, RainLevelChange, ThunderLevelChange}`, `ClientboundAddEntity`, `ClientboundSound` (`WTH-NET-001`) |
| `mcrs_minecraft_engine_world` | 4 | `HasWeather` marker component on weather-capable dimensions |
| `mcrs_minecraft_engine` | 4 | `ServerEdit::{ClearWeather, RainWeather, ThunderWeather}`, `Weather` host resource, `WorldStatePlugin` load/autosave, `DimCells`/`DimInput` sync, `enter_level_packets` / join weather hook, `PacketTarget::AllInDim` |
| `mcrs_minecraft_command_world` | 5 | `/weather clear|rain|thunder [<duration>]` emitting `ServerEdit` and `commands.weather.set.{clear,rain,thunder}` (`CMD-W-001`..`005`) |
| `mcrs_minecraft_vanilla_weather` (new) | 5 | `VanillaWeatherPlugin` on `HasWeather` dimensions: startup level init (`1.0`/`0.0`), per-tick `WeatherLevel` ramp, broadcast-once `Bytes` packet fanout, join sync, and lightning strike pass |
| `mcrs_minecraft_server` | 7 | `VanillaWeatherPlugin` registered into dimensions via `AddDimensionPlugins` in `MinecraftServerPlugins` |

Reason for `#![no_std]` `mcrs_minecraft_weather`: client prediction, bots, replay tools, and
dimension sub-apps share the `f32` `0.01`/tick ramp arithmetic, `> 0.2` wet predicate, and cycle
timer rules without pulling in Bevy, disk I/O, or engine registries.

## 3. Host-dimension sync and command edits

1. Host startup (`WorldStatePlugin::load_world_state`) reads `data/minecraft/weather.dat` into the
   host `Weather` resource (`max(0)` clamp on loaded timers).
2. Host tick (`ServerStateHostPlugin`, `FixedUpdate`, gated on `tick_runs_normally` and
   `GameRule::AdvanceWeather`) advances `WeatherState` timers using `mcrs_minecraft_weather`
   (`WTH-CYC-002`, `WTH-CYC-003`). When `advance_weather` is false, all five fields stay frozen.
3. `DimCells` forwards `changed::<Weather>` to each dimension `SubApp` in `DimInput`, alongside
   `GameRules`.
4. `/weather clear|rain|thunder [<duration>]` in `mcrs_minecraft_command_world` produces
   `ServerChange(ServerEdit::*)`, routed through `DimMail::edit_server` -> `DimOutput::server_edits`
   -> `apply_host_edit` on the host, returning `<duration>` or `-1` (`CMD-W-002`..`005`).

## 4. Per-dimension tick, network delivery, and lightning

1. **Startup initialization (`WTH-LVL-003`)**: on the first tick after world load in a `HasWeather`
   dimension, `WeatherLevel` initializes directly from host `Weather` (`1.0` if true, `0.0` if
   false), avoiding a startup ramp across restarts (`join_alice_after_restart`).
2. **Per-tick ramp and `Bytes` broadcast (`WTH-LVL-001`, `WTH-LVL-002`, `WTH-NET-002`)**: in
   `TickSet::Project` (running on `HasWeather` dimensions whenever `tick_runs_normally`, even when
   `advance_weather` is false), `VanillaWeatherPlugin` steps `WeatherLevel` by `±0.01f32` toward
   `Weather.raining` and `Weather.thundering`. On any changed field (`rain`, `thunder`, or
   `wet = rain > 0.2` threshold crossing), it pre-encodes each `ClientboundGameEvent` once into a
   shared `Bytes` payload (or `PacketPayload::GameEvent` via `bridge_outbound::encode_once` for
   `PacketTarget::AllInDim`) so `N` players in the dimension reuse one wire buffer.
3. **Player join and dimension entry (`WTH-NET-003`)**: when a player enters a `HasWeather`
   dimension with `wet == true` (`rain > 0.2`), the sync system writes `[BeginRaining(0.0),
   RainLevelChange(rain), ThunderLevelChange(thunder)]` to `OutboundPlayerPacket` before
   `LevelChunksLoadStart`. Non-weather dimensions (Nether, End, `has_ceiling`) lack `HasWeather` and
   emit no weather packets.
4. **Lightning pass (`WTH-LGT-001`..`003`)**: in `TickSet::WorldWrite` on `HasWeather` dimensions
   during active thunder (`wet && thunder > 0.2`), each active ticked column chunk rolls
   `1 / 100_000`, picks a sky-exposed heightmap column, redirects to any `minecraft:lightning_rod`
   within `128` blocks, verifies rain precipitation at the target, and spawns
   `minecraft:lightning_bolt` with `thunder`/`impact` sounds, fire ignition, and `5` base damage.
