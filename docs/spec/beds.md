# Beds and sleep

Version: see `assets/minecraft/version.json`.

| Field | Value |
|-------|-------|
| Status | `draft` |

This document states the rules for beds, sleep checks, spawn updates, sleep progress, night skips, and bed explosions.

## 0. Conventions, scope, and corpus

- **Bed block**: a block in tag `beds` or block `straw_bed`. **Head** and **foot**: `part` is `head` or `foot`.
- **Partner block**: the head block for a foot block, or the foot block for a head block.
- **Bed rule**: the value of attribute `gameplay/bed_rule` or `gameplay/straw_bed_rule`.
- **Sleeper**: a player who rests in a bed. **Deep sleep**: a sleeper with `SleepTimer >= 100`.
- **Sleeping position**: entity data entry 14 that holds the head block coordinates (`player-pose.md`).
- **Sky light level**: the effective float value of attribute `gameplay/sky_light_level`.
- **Sleep rejection**: a failed attempt to enter sleep after a player uses a bed block.
- **Scope**: pose changes belong to `player-pose.md`; bunk respawn order belongs to `player-death.md`; explosion damage belongs to `explosions.md`; villager AI is out of scope.
- **Artefacts**: `tools/mcrs_minecraft_probe/traces/beds.json`, `data/minecraft/tags/block/beds.json`, `data/minecraft/timeline/day.json`, `data/minecraft/dimension_type/{overworld,the_nether,the_end}.json`, `assets/mcrs/block_definition/{red_bed,straw_bed}.json`, `assets/mcrs/registry_definition/game_rule.json`, `assets/mcrs/reports/packets.json`, and `environment.json`.

## 1. Two-part geometry and block states

**BED-GEO-001** (`black_box_confirmed`, semantic, confidence: high). Every bed block has three properties: `facing` (`north`, `south`, `west`, `east`), `part` (`foot`, `head`), and `occupied` (`true`, `false`). Dyed beds appear in tag file `data/minecraft/tags/block/beds.json` with height `0.5625` (`red_bed.json`). That tag file omits `straw_bed`, which has base height `0.25` (`straw_bed.json`; `beds.json`).

**BED-GEO-002** (`black_box_confirmed`, semantic, confidence: high). The head block sits one block from the foot block along `facing` (`beds.json` `bed_place_and_break`; `red_bed.json`, `straw_bed.json`). Offsets are: `north` `(0, -1)`, `south` `(0, +1)`, `west` `(-1, 0)`, `east` `(+1, 0)`.

**BED-GEO-003** (`black_box_confirmed`, semantic, confidence: high). Bed placement succeeds only when both foot and head positions accept replacement blocks (`beds.json` `bed_place_and_break`). Both placed halves share `facing` and start with `occupied` set to `false`.

**BED-GEO-004** (`black_box_confirmed`, semantic, confidence: high). When either half breaks, the server removes the partner half (`beds.json` `bed_place_and_break`). Sleep start sets `occupied` to `true` on both halves; sleep exit sets `false` (`beds.json` `clear_rain_thunder_when_dark`, `multiplayer_sleep_percentage_and_morning_skip`).

**BED-GEO-005** (`black_box_confirmed`, wire, confidence: high). A sleeper rests horizontally at the head center and vertically at `head_y + top_height + 0.125`. Dyed beds use `head_y + 0.6875`, and `straw_bed` uses `head_y + 0.375`. The sleeping position holds the head coordinates (`beds.json`; `player-pose.md`, POSE-SRV-006).

## 2. Dimension bed rules and sky-light condition

**BED-RUL-001** (`black_box_confirmed`, semantic, confidence: high). Dyed beds read `gameplay/bed_rule`, and `straw_bed` reads `gameplay/straw_bed_rule` (`dimension_type/*.json`, `environment-attributes.md`, `beds.json` `daytime_spawn_and_reject`, `sleep_timer_straw_bed_and_break_wake`, `nether_and_end_explosions`):

| Dimension | Attribute | `can_sleep` | `can_set_spawn` | `destroy_on_use` | `destroy_on_leave` |
|-----------|-----------|-------------|-----------------|------------------|--------------------|
| `overworld` | `bed_rule` | `when_dark` | `always` | `false` | `false` |
| `overworld` | `straw_bed_rule` | `when_dark` | `never` | `false` | `true` |
| `the_nether`, `the_end` | both | `never` | `never` | `true` | `false` |

**BED-RUL-002** (`black_box_confirmed`, semantic, confidence: high). Under `when_dark`, sleep requires sky light level `v < 11.0`. Start from `day` light `v = 15.0 * factor` (`timeline/day.json`). Apply `R` (rain) and `T = R * thunder` (`ENVA-LAYER-005`, `environment.json`):
- Let `r = R - T`. When `r > 0`, `v = lerp(r, v, alpha_blend(v, 4.0, 0.3125))`.
- Then when `T > 0`, `v = lerp(T, v, alpha_blend(v, 4.0, 0.52734375))`.

**BED-RUL-003** (`black_box_confirmed`, semantic, confidence: high). On timeline `day`, `when_dark` matches clear ticks `12523..=23477`, full-rain ticks `12002..=23998`, and all ticks under full storm (`beds.json` `clear_rain_thunder_when_dark`).

## 3. Sleep eligibility, spawn updates, and explosions

**BED-SLP-001** (`source_suggested_black_box_confirmed`, semantic, confidence: high). When `destroy_on_use` is `true`, bed use destroys both halves, creates fire, and triggers an explosion. The explosion has power `5.0` at the head center (`explosions.md`). Trace `beds.json` confirms fire, removed bed halves, and blast damage from `[0.5, 64.5, 3.5]` in Nether and `[100.5, 70.5, 3.5]` in End.

**BED-SLP-002** (`black_box_confirmed`, semantic, confidence: high). When `destroy_on_use` is `false` and `occupied` is `true`, sleep fails with no respawn change (`beds.json` `multiplayer_sleep_percentage_and_morning_skip`).

**BED-SLP-003** (`black_box_confirmed`, semantic, confidence: high). On an unoccupied bed with `destroy_on_use` set to `false`, `can_set_spawn` controls respawn updates. When allowed, bed use sets respawn to the head block even in daylight (`beds.json` `daytime_spawn_and_reject`).

**BED-SLP-004** (`black_box_confirmed`, semantic, confidence: high). Sleep fails when the player lies outside `(3.0, 2.0, 3.0)` of both the foot block center and the head block center (`beds.json` `obstruction_reach_and_monsters`).

**BED-SLP-005** (`black_box_confirmed`, semantic, confidence: high). Sleep fails when an opaque solid block occupies the space above the head block or the foot block. Transparent blocks such as glass permit sleep (`beds.json` `obstruction_reach_and_monsters`).

**BED-SLP-006** (`black_box_confirmed`, semantic, confidence: high). In non-creative modes, sleep fails when a hostile mob box strictly intersects box `(+/-8.0, +/-5.0, +/-8.0)`. That search box centers on `(head_x + 0.5, head_y, head_z + 0.5)`. Creative mode skips this check (`beds.json`).

## 4. Sleep progress, wake triggers, and night skip

**BED-SKP-001** (`black_box_confirmed`, semantic, confidence: high). Each tick in bed increments `SleepTimer` by `1` up to `100`, where the sleeper enters deep sleep (`beds.json` `sleep_timer_straw_bed_and_break_wake`).

**BED-SKP-002** (`black_box_confirmed`, semantic, confidence: high). Sleep ends on `stop_sleeping` (`player-pose.md`, POSE-SRV-006), night skip, absent bed block, or failed `can_sleep` (POSE-SRV-016). Except on an absent bed block, sleep exit destroys both halves when `destroy_on_leave` is `true` (`beds.json` `sleep_timer_straw_bed_and_break_wake`).

**BED-SKP-003** (`black_box_confirmed`, semantic, confidence: high). Game rule `players_sleeping_percentage` (default `100`, minimum `0`, `game_rule.json`) controls night skip for `N` non-spectator players. Skip never occurs when `N == 0` or `percentage > 100`. Otherwise, `required = max(1, ceil(N * max(percentage, 0) / 100))` deep sleepers skip the night (`beds.json`).

**BED-SKP-004** (`black_box_confirmed`, semantic, confidence: high). When enough sleepers reach deep sleep, all sleepers wake (`occupied` becomes `false`). With `advance_time` `true`, day time jumps to the next multiple of `24000` (`wake_up_from_sleep`). With `advance_weather` `true`, active rain or thunder clears (`beds.json` `multiplayer_sleep_percentage_and_morning_skip`).

## Experiments

1. Run `beds.json` (`26.4-snapshot-7`). Place a south `red_bed`, block head placement, break foot, then use beds at noon. Observed: placement blocks when head space is full, and break clears both halves. Block `red_bed` sets `respawn` at noon, and `straw_bed` does not.
2. Test clear, rain, and storm ticks `0`, `6000`, `12000`, `18000`, then test foot-only, head-only, and over-limit reach, obstructions, and zombies. Observed: sleep holds at clear `12523..=23477`, rain `12002..=23998`, and storm `0`, `6000`, `12000`, `18000`. It holds at foot-only and head-only reach `3.0`, glass, outside `8x5x8`, and creative mode.
3. Sleep in `straw_bed` and exit; run percentages `100`, `50`, and `0`; use `red_bed` at Nether `[0, 64, 2]` and End `[100, 70, 2]`. Observed: `straw_bed` sleeps at `0.375` and breaks on exit, and skip follows `BED-SKP-003` and clears rain. Both Nether and End remove both bed halves, spawn fire, and deal blast damage from `[0.5, 64.5, 3.5]` and `[100.5, 70.5, 3.5]`.

## Unresolved

None.
