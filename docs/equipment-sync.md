# Player equipment sync: design

Design note for server-side replication of the six player equipment slots (`mainhand`, `offhand`,
`feet`, `legs`, `chest`, `head`) and equip sounds. Behaviour comes from `docs/spec/equipment-sync.md`
(`EQP-*`), `docs/spec/client-entities.md` (`CENTS-EQUIP-001`), `docs/spec/inventory.md` (`INV-SLT-001`,
`INV-SEL-001`), and `docs/spec/arm-swing.md` (`SWG-DST-003`).

## 1. Classification of the state

| Value | Kind | Owner |
|-------|------|-------|
| `Slots`, `Selected` on a player | authoritative truth (`0..=8`, `36..=39`, `40`) | `mcrs_minecraft_engine_item`; mutated by inventory requests and `apply_selected_slot` |
| `SlotChanged` message | temporal fact (`SlotRef`) | `mcrs_minecraft_engine_item` (`InSlot` hooks and engine stack commands) |
| `PlayerEquipment<T>` snapshot (`[Option<T>; 6]`) | materialized baseline for diff and late-viewer spawn | `EntityCodec` (`Subject` in `mcrs_minecraft_replication`) for wire `ProtoStack<Wire>`; sparse baseline in `mcrs_minecraft_vanilla_equipment` |
| `DirtyEquipment` work queue (`Vec<Entity>`) | temporal work queue (empty after flush) | `mcrs_minecraft_vanilla_equipment`, populated only from `SlotChanged`, `Changed<Selected>`, and `Added<Replicated>` |
| `PlayerData::Equipment` / `PlayerData::SwapHands` | per-tick replication outbox | pushed into `TileEvents`; `EntityCodec` encodes one `Bytes` buffer and shares it across observers |

## 2. Crate map

| Crate | Layer | Content |
|-------|-------|---------|
| `mcrs_minecraft_equipment` (`#![no_std]`, new) | 3, rules | `EquipmentSlots` (6-slot snapshot `[Option<S>; 6]`), slot index mapping (`mainhand`/`offhand`/`feet`/`legs`/`chest`/`head`), `diff_equipment`, `spawn_entries`, `is_hand_swap` |
| `mcrs_minecraft_inventory_player` (`#![no_std]`) | 3, rules | `HOTBAR`, `OFF_HAND`, `ArmourSlot` (`Feet` = `36`, `Legs` = `37`, `Chest` = `38`, `Head` = `39`) |
| `mcrs_minecraft_protocol_game` | 3, data | `EquipmentSlot`, `ClientboundSetEquipment` (ID `104`), `ClientboundEntityEvent` (`SWAP_HANDS = 55`, ID `34`) |
| `mcrs_minecraft_replication` | 3, data | `PlayerSpawn::equipment`, `PlayerData::Equipment`, `PlayerData::SwapHands`, `Subject` baseline, `EntityCodec` spawn-bundle emission and broadcast-once `Bytes` fan-out excluding owner |
| `mcrs_minecraft_engine_item` | 4 | `Slots`, `Selected`, `SlotChanged`, `Stacks` |
| `mcrs_minecraft_engine` | 4 | `Replicated`, `TileEvents`, `publish_player_tiles`, `MuxerStage` (`views_out` -> `Entities` -> `changes_out`) |
| `mcrs_minecraft_vanilla_equipment` (new) | 5 | `VanillaEquipmentPlugin`: sparse listeners on `SlotChanged`, `Changed<Selected>`, `Added<Replicated>`, equippable sound dispatch (`EQP-SND-001`) |
| `mcrs_minecraft_vanilla_inventory` | 5 | `VanillaInventoryPlugin` (unchanged dependency graph; zero protocol/network edges) |
| `mcrs_minecraft_server` | 7 | `VanillaEquipmentPlugin` registered in `MinecraftServerPlugins` |

Reason for `mcrs_minecraft_equipment` (`#![no_std]` with `alloc`, optional `bevy` feature off by
default): bots, proxies, replay viewers, and tests share the 6-slot indexing, spawn filtering, and
main/offhand swap detection without Bevy, tokio, or engine item tables. Both new crates require an
RFD before their implementation PR (`CONTRIBUTING.md`, `AGENTS.md`).

## 3. Use cases

| User | Takes | Does not take |
|------|-------|---------------|
| proxy, replay viewer | `mcrs_minecraft_protocol_game`, `mcrs_minecraft_equipment` | Bevy, server engine, inventory plugins |
| headless bot | `mcrs_minecraft_equipment`, `mcrs_minecraft_inventory_player` | rendering, server replication |
| minigame server | `mcrs_minecraft_engine`, `mcrs_minecraft_engine_item`; writes `PlayerData::Equipment` or replaces `VanillaEquipmentPlugin` | vanilla equip sounds or default slot mapping |

## 4. Inventory-edge boundary and ECS rules

- **Zero `INVENTORY_EDGE_PREFIXES` violations**: `xtask/src/checks.rs` forbids
  `mcrs_minecraft_vanilla_inventory` from depending on `mcrs_minecraft_network`,
  `mcrs_minecraft_protocol*`, `mcrs_minecraft_inventory_click`, `mcrs_minecraft_inventory_client`,
  or `mcrs_minecraft_inventory_server`. Placing equipment sync in `mcrs_minecraft_vanilla_equipment`
  (sibling to `mcrs_minecraft_vanilla_swing`) leaves `mcrs_minecraft_vanilla_inventory` decoupled;
  mutations flow solely through `SlotChanged` and `Selected`.
- **No `&mut World`**: every system in `mcrs_minecraft_vanilla_equipment` uses typed `Query`, `Res`,
  `ResMut`, `MessageReader`, `Stacks`, and `RegistryNames` (`mcrs::exclusive_system` clean).
- **Sparse work and zero steady-state tick allocation**:
  1. `collect_slot_changes` (`run_if(on_message::<SlotChanged>)`) maps `SlotRef` via `PlayerHolders`
     and `Selected` in `O(1)` per event; slots `9..=35` and unselected hotbar slots are ignored.
  2. `collect_selection_changes` (`run_if(any_match_filter::<Changed<Selected>>)`) queues players
     whose `Selected` changed (`select_slot_1`, `select_slot_0`).
  3. `publish_player_tiles` (or `Added<Replicated>` hook) supplies initial non-empty equipment on
     `PlayerSpawn` (`EQP-SYN-001`).
  4. `flush_equipment` (`run_if(has_dirty_equipment)`, in `FixedUpdate` after `TickSet::Project` /
     `InventorySet::Drops` and before `FixedPostUpdate` `publish_player_tiles`) converts changed
     slots via `mcrs_minecraft_item_stack::write_wire` using a `Local<Vec<u8>>` scratch buffer, runs
     `diff_equipment`, and emits `PlayerData::SwapHands` (`EQP-SYN-005`) or `PlayerData::Equipment`
     into `TileEvents`.
- **Broadcast-once `Bytes` and observer ordering**: in `EntityCodec::finish_tick`, `send_swings`
  runs before `send_equipment` (`EQP-SYN-002`, `SWG-DST-003`). `EntityCodec` records one
  `ClientboundSetEquipment` or `ClientboundEntityEvent` (`55`) into `Bytes`, updates `Subject`'s
  6-slot baseline (so `spawn_chunks` serves late viewers like `Charlie` in `EQP-E5`), and clones
  that `Bytes` handle to every `ViewerId` in `self.holds.holders(subject)` except `state.viewer`
  (`EQP-SYN-003`, `EQP-SYN-004`). Equip sounds (`EQP-SND-001`) emit when an equippable enters
  `feet..=head`, before the tick's `TileEvents` equipment packet.
