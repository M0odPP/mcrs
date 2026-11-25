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
| `OwnEquipment` (`EquipmentSlots<ProtoStack<Wire>>`) on `Player` | materialized 6-slot wire snapshot (mirrors `OwnPoseEntries`) | registered as required on `Player` in `mcrs_minecraft_engine`; written by `mcrs_minecraft_vanilla_equipment` before `FixedPostUpdate`; read by `publish_player_tiles` for `PlayerSpawn` |
| `DirtyEquipment` work queue (`Vec<Entity>`) | temporal work queue (empty after flush) | `mcrs_minecraft_vanilla_equipment`, populated only from `Added<PlayerHolders>`, `SlotChanged`, and `Changed<Selected>` |
| `PlayerData::Equipment` / `PlayerData::SwapHands` | per-tick replication outbox | pushed into `TileEvents` only when `Replicated` exists; `EntityCodec` encodes one `Bytes` buffer and shares it across observers |
| `Subject::equipment` in `EntityCodec` | materialized replication baseline | `mcrs_minecraft_replication`; initialized from `PlayerSpawn::equipment` in `EntityCodec::join` and updated on `Equipment` / `SwapHands` |
| `PacketPayload::Sound(ClientboundSound)` | per-recipient sound packet (`EQP-SND-001`) | emitted into `OutboundPlayerPacket` for actor and nearby viewers on armour equip |

## 2. Crate map

| Crate | Layer | Content |
|-------|-------|---------|
| `mcrs_minecraft_equipment` (`#![no_std]`, new) | 3, rules | `EquipmentSlots<S>` (6-slot snapshot `[Option<S>; 6]`), slot mapping (`mainhand`/`offhand`/`feet`/`legs`/`chest`/`head`), `diff_equipment`, `spawn_entries`, `is_hand_swap` |
| `mcrs_minecraft_inventory_player` (`#![no_std]`) | 3, rules | `HOTBAR`, `OFF_HAND`, `ArmourSlot` (`Feet` = `36`, `Legs` = `37`, `Chest` = `38`, `Head` = `39`) |
| `mcrs_minecraft_protocol_game` | 3, data | `EquipmentSlot`, `ClientboundSetEquipment` (ID `104`), `ClientboundEntityEvent` (`SWAP_HANDS = 55`, ID `34`), `ClientboundSound` (ID `119`, new) |
| `mcrs_minecraft_replication` | 3, data | `PlayerSpawn::equipment`, `PlayerData::Equipment`, `PlayerData::SwapHands`, `Subject` baseline initialized on `join`, `EntityCodec` spawn-bundle emission and broadcast-once `Bytes` fan-out excluding owner |
| `mcrs_minecraft_engine_item` | 4 | `Slots`, `Selected`, `SlotChanged`, `Stacks` |
| `mcrs_minecraft_engine` | 4 | `OwnEquipment` (required on `Player`), `Replicated`, `TileEvents`, `publish_player_tiles`, `PacketPayload::Sound` / `OutboundPlayerPacket` (`bus.rs`), `MuxerStage` (`views_out` -> `Entities` -> `changes_out`) |
| `mcrs_minecraft_vanilla_equipment` (new) | 5 | `VanillaEquipmentPlugin`: sparse listeners on `Added<PlayerHolders>`, `SlotChanged`, `Changed<Selected>`, updating `OwnEquipment`, emitting `TileEvents`, and dispatching `PacketPayload::Sound` (`EQP-SND-001`) |
| `mcrs_minecraft_vanilla_inventory` | 5 | `VanillaInventoryPlugin` (unchanged dependency graph; zero protocol/network edges) |
| `mcrs_minecraft_server` | 7 | `VanillaEquipmentPlugin` registered in `MinecraftServerPlugins` |

Reason for `mcrs_minecraft_equipment` (`#![no_std]` with `alloc`, optional `bevy` feature off by
default): bots, proxies, replay viewers, and tests share the 6-slot indexing, spawn filtering, and
hand-swap detection without Bevy, tokio, or engine item tables. Both new crates require an RFD
before their implementation PR (`CONTRIBUTING.md`, `AGENTS.md`).

## 3. Use cases

| User | Takes | Does not take |
|------|-------|---------------|
| proxy, replay viewer | `mcrs_minecraft_protocol_game`, `mcrs_minecraft_equipment` | Bevy, server engine, inventory plugins |
| headless bot | `mcrs_minecraft_equipment`, `mcrs_minecraft_inventory_player` | rendering, server replication |
| minigame server | `mcrs_minecraft_engine`, `mcrs_minecraft_engine_item`; writes `OwnEquipment` / `PlayerData::Equipment` or replaces `VanillaEquipmentPlugin` | vanilla equip sounds or default slot mapping |

## 4. Inventory-edge boundary, spawn ordering, and ECS rules

- **Zero `INVENTORY_EDGE_PREFIXES` violations**: `xtask/src/checks.rs` forbids
  `mcrs_minecraft_vanilla_inventory` from depending on `mcrs_minecraft_network`,
  `mcrs_minecraft_protocol*`, `mcrs_minecraft_inventory_click`, `mcrs_minecraft_inventory_client`,
  or `mcrs_minecraft_inventory_server`. Placing equipment sync in `mcrs_minecraft_vanilla_equipment`
  (sibling to `mcrs_minecraft_vanilla_swing`) keeps `mcrs_minecraft_vanilla_inventory` decoupled;
  mutations flow solely through `SlotChanged` and `Selected`, and `mcrs_minecraft_engine` needs no
  dependency on `mcrs_minecraft_engine_item` because it holds only `OwnEquipment`.
- **No `&mut World`**: every system in `mcrs_minecraft_vanilla_equipment` uses typed `Query`, `Res`,
  `ResMut`, `MessageReader`, `MessageWriter`, `Stacks`, and `RegistryNames` (`mcrs::exclusive_system`
  clean).
- **Spawn ordering (no duplicate tick-1 packet)**: `publish_player_tiles` (in `FixedPostUpdate`)
  emits `PlayerData::Spawn(PlayerSpawn { .. })` into `TileEvents` before queuing
  `commands.entity(player).insert(Replicated { .. })`. Mirroring `OwnPoseEntries` (`publish_pose.rs`),
  `VanillaEquipmentPlugin` runs in `FixedUpdate` (which always executes before `FixedPostUpdate` in
  `FixedMain`), triggered sparsely on `Added<PlayerHolders>`, `SlotChanged`, and `Changed<Selected>`:
  1. `collect_slot_changes` (`run_if(on_message::<SlotChanged>)`) filters `SlotRef` via `PlayerHolders`
     and `Selected` in `O(1)` (`0..=8`, `36..=39`, `40`); `P[9]..=P[35]` and unselected hotbar
     slots are ignored (`EQP-SLT-001`).
  2. `collect_selection_and_init` (`run_if(any_match_filter::<Or<(Changed<Selected>, Added<PlayerHolders>)>>)`)
     queues players whose `Selected` changed or whose holders just attached.
  3. `flush_equipment` (`run_if(has_dirty_equipment)`, in `FixedUpdate` after `TickSet::Project` /
     `InventorySet::Drops`) encodes changed slots via `mcrs_minecraft_item_stack::write_wire` into a
     `Local<Vec<u8>>` scratch buffer, runs `diff_equipment` against `OwnEquipment`, updates
     `OwnEquipment` via `set_if_neq`, and emits `PlayerData::SwapHands` / `PlayerData::Equipment`
     into `TileEvents` **only when `Replicated` is already present**.
  4. When `Replicated` is absent (initial join/respawn), `OwnEquipment` already holds the snapshot
     when `publish_player_tiles` reads it into `PlayerSpawn::equipment`, and `EntityCodec::join`
     seeds `Subject::equipment` from `spawn.equipment` so tick 1 sends zero duplicate packets.
- **Broadcast-once `Bytes` and observer ordering**: in `EntityCodec::finish_tick`, `send_swings`
  runs before `send_equipment` (`EQP-SYN-002`, `SWG-DST-003`). `EntityCodec` encodes one
  `ClientboundSetEquipment` or `ClientboundEntityEvent` (`55`) into `Bytes`, updates
  `Subject::equipment` (so `spawn_chunks` serves late viewers like `Charlie` in `EQP-E5`), and
  clones that `Bytes` handle to every `ViewerId` in `self.holds.holders(subject)` except
  `state.viewer` (`EQP-SYN-003`, `EQP-SYN-004`). Equip sounds (`PacketPayload::Sound`,
  `EQP-SND-001`) emit to actor and nearby viewers before the tick's `TileEvents` equipment packet.
