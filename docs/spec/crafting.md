# Crafting and Smelting: behavioural specification

Version: see `assets/minecraft/version.json`. Wire packet IDs: `assets/mcrs/reports/packets.json`.

Out of scope: menu slots and click modes (`docs/spec/inventory.md`), transaction state (`docs/spec/inventory-transactions.md`), and item data (`assets/mcrs/item_definition/`).

## 0. Conventions

- **Crafting**: Conversion of `Crafting grid` inputs into output in the 2x2 inventory grid or 3x3 table grid.
- **Smelting**: Timed heat transformation of input items inside a `Furnace station`.
- **Remainder**: Item placed into a consumed input slot or player inventory after a craft or fuel burn.
- **Mirroring**: Horizontal row reversal of a normalized shaped recipe pattern.
- **Transmute**: Recipe that converts one base stack while it copies explicit data components onto the result.
- **Crafting grid**: Input slots `1..=4` of inventory or slots `1..=9` of a `crafting_table`.
- **Furnace station**: One of `furnace`, `smoker`, or `blast_furnace`.
- **Burn left**: Property `0` of a `Furnace station` that counts active fuel ticks left.
- **Burn total**: Property `1` of a `Furnace station` that holds the duration of the last ignited fuel.
- **Cook total**: Property `3` of a `Furnace station` that holds ticks required per output item.
- Prefixes: `CRF-GRD`, `CRF-SHP`, `CRF-SHL`, `CRF-TRN`, `CRF-REM`, `CRF-SPC`, `CRF-RBK`, `CRF-SML`, `CRF-FUL`, `CRF-EXP`, `CRF-E`, `CRF-U`.

## 1. Crafting grid and recipes

- **CRF-GRD-001** (provenance: `black_box_confirmed`, kind: `wire`, confidence: `high`, experiment: `CRF-E1`). Player inventory must provide a 2x2 `Crafting grid` in slots `1..=4` with output in slot `0`. A `crafting_table` must provide a 3x3 `Crafting grid` in slots `1..=9` with output in slot `0`.
- **CRF-GRD-002** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). Each edit to a `Crafting grid` slot must recompute slot `0`. On `pickup` click of slot `0` with an empty cursor, the server must move one result stack to the cursor. It must also decrement each active input slot by `1`.
- **CRF-GRD-003** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). A `quick_move` click on slot `0` must place the result in player inventory and consume one input set. When a player closes a `crafting_table`, the server must return unspent items in slots `1..=9` to player inventory.
- **CRF-SHP-001** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). A shaped recipe must match at any `(offset_x, offset_y)` inside the `Crafting grid`. Every grid cell outside that pattern box must be empty.
- **CRF-SHP-002** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). When a shaped recipe allows `Mirroring`, it must also match horizontal row reversal of its pattern.
- **CRF-SHP-003** (provenance: `documentation`, kind: `semantic`, confidence: `medium`). Shaped pattern normalization must trim empty outer rows and columns while it keeps empty interior rows (per `acacia_shelf` recipe data).
- **CRF-SHL-001** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). A shapeless recipe must match only when the non-empty slot count equals the ingredient count under one-to-one assignment.
- **CRF-TRN-001** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). A `Transmute` recipe must match one base input and material input, convert to the target item, and copy explicit data components (`minecraft:custom_name`) onto the result.
- **CRF-TRN-002** (provenance: `documentation`, kind: `semantic`, confidence: `medium`). When `"result"` omits `"id"`, `Transmute` must keep the base input item (per `map_cloning` recipe data). When `add_material_count_to_result` is true, output count must add material slots clamped to result max stack size.
- **CRF-REM-001** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). An input with `crafting_remainder` (`milk_bucket` to `bucket`, `honey_bottle` to `glass_bottle`) must replace single-item consumed slots in place. When input count exceeds `1`, the `Remainder` must move to player inventory.
- **CRF-SPC-001** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). Armor color craft must combine dyeable armor (`leather_helmet`) with `red_dye` and set `minecraft:dyed_color` (`11546150`) on the output item.
- **CRF-SPC-002** (provenance: `documentation`, kind: `semantic`, confidence: `medium`). Multi-dye armor mix must sum red, green, and blue channels across dyes and any present `minecraft:dyed_color`. It must scale channel means by `average_max / max_average` clamped to `0..=255`.
- **CRF-SPC-003** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). Tool repair must match two damaged tools of one item kind (`iron_pickaxe`, damage `200` of `250`) and output one tool with damage `138` (`250 - (50 + 50 + floor(250 * 0.05))`).

## 2. Recipe book and smelting

- **CRF-RBK-001** (provenance: `black_box_confirmed`, kind: `wire`, confidence: `high`, experiment: `CRF-E1`). On join, the server must send `update_recipes` (`9` sets), `recipe_book_settings` (four books `false`), `recipe_book_add` (`replace = true`), and unlocked entries. Pickups and `/recipe` commands must emit `recipe_book_add` and `recipe_book_remove`, and `recipe_book_change_settings` must persist in saved player data.
- **CRF-SML-001** (provenance: `black_box_confirmed`, kind: `wire`, confidence: `high`, experiment: `CRF-E1`). A `Furnace station` menu must map slot `0` to input, slot `1` to fuel, and slot `2` to output. Container data properties `0..=3` must report `Burn left`, `Burn total`, cook progress, and `Cook total`.
- **CRF-SML-002** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). A valid recipe in slot `0` must set `Cook total` (`200` on `furnace`, `100` on `smoker` and `blast_furnace`). While `Burn left > 0` and the input stays valid, each tick must decrement `Burn left` by `1` and advance cook progress by `1`.
- **CRF-SML-003** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). When `Burn left` hits `0` without fuel re-ignition, cook progress must decay by `2` per tick down to `0` on that tick (`100 -> 98 -> 96 -> 94`).
- **CRF-FUL-001** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). Fuel ignition must consume `1` fuel from slot `1` and set `Burn left = Burn total`. On `smoker` and `blast_furnace`, `Burn total` must halve base fuel burn duration (`stick`: `100` to `50`; `coal` or `charcoal`: `1600` to `800`).
- **CRF-FUL-002** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). On a `1 -> 0` `Burn left` transition with a valid recipe and fuel in slot `1`, re-ignition must occur on that tick with zero cook decay. Ignition of `lava_bucket` (`Burn total = 20000`) must leave `bucket` in slot `1`, and `wet_sponge` completion must turn `bucket` into `water_bucket`.
- **CRF-EXP-001** (provenance: `black_box_confirmed`, kind: `semantic`, confidence: `high`, experiment: `CRF-E1`). Output extraction from `furnace` slot `2` must grant player experience (`1` for one `gold_ore`) and clear `RecipesUsed` on the block entity.

## 3. Experiments

- **CRF-E1** (`tools/mcrs_minecraft_probe/scenarios/crafting.json` -> `tools/mcrs_minecraft_probe/traces/crafting.json`):
  1. Connect `crfprobeplyr`, then grant `oak_log` and run `recipe take` and `recipe give`.
  2. Send `recipe_book_change_settings`, then craft `oak_planks` and shifted `stick` in the 2x2 `Crafting grid`.
  3. Open a `crafting_table` and craft shifted `torch`, mirrored `oak_stairs`, `red_shulker_box`, dyed `leather_helmet`, repaired `iron_pickaxe`, `cake`, and `sugar`.
  4. Freeze ticks, then smelt `gold_ore` and `wet_sponge` with `stick`, `coal`, `charcoal`, and `lava_bucket` across all three `Furnace station` blocks.
  5. Extract `gold_ingot`, query `XpTotal` and `RecipesUsed`, and save player data.
  - Result: recipe book packets, slot packets, NBT components, ticks, and experience match all rules backed by `CRF-E1` (`CRF-SHP-003`, `CRF-TRN-002`, `CRF-SPC-002` are documentation rules).

## 4. Unresolved

- **CRF-U1**: Specialized recipe serializers (firework, banner copy, book copy, map extension, shield banner, decorated pot, tipped arrow) are not probed.
- **CRF-U2**: Repeated multi-craft `quick_move` up to stack limits and `pickup` onto a compatible non-empty cursor on slot `0` are not probed.
- **CRF-U3**: Ground drop behavior when a `Remainder` overflows a full player inventory is not probed.
- **CRF-U4**: Transmute with `"result": {}` (`map_cloning`), `add_material_count_to_result`, and multi-dye armor color mix are not probed in `CRF-E1`.
- **CRF-U5**: Custom component reset on tool repair and enchanted tool repair rejection vs grindstone are not probed in `CRF-E1`.
- **CRF-U6**: `limited_crafting` game rule behavior and `recipe_book_add` notification flags are not probed.
- **CRF-U7**: Fractional furnace experience accumulation and uniform random extra point over multi-item output stacks are not probed (`CRF-E1` probes `1.0`).
- **CRF-U8**: Mid-burn `Cook total` updates when slot `0` switches to a recipe with a different cook time are not probed in `CRF-E1`.
