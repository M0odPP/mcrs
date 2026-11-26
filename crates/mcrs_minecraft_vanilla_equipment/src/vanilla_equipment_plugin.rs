use std::sync::Arc;

use bevy_app::{App, FixedPostUpdate, FixedUpdate, Plugin};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{Message, MessageReader, MessageWriter, Messages};
use bevy_ecs::prelude::{
    Added, Changed, Commands, Has, Local, Populated, Query, Res, ResMut, Resource, With,
};
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy_ecs::world::World;
use mcrs_engine_muxer::{TileKey, View};
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketTarget};
use mcrs_minecraft_engine::loaded_client::UnloadedClient;
use mcrs_minecraft_engine::swing::{SwingPlayer, SwingSimulateSet};
use mcrs_minecraft_engine::view_radius::VIEW_RADIUS;
use mcrs_minecraft_engine::{
    BlockBreakAckSet, OwnEquipment, PlayerEquipmentDelta, PlayerEquipmentSyncSet,
    PublishPlayerTilesSet, UseItemRequest,
};
use mcrs_minecraft_engine_item::{
    GateQuery, HolderCommandsExt, ItemAppExt, ItemCommandsExt, Request, RequestId, RequestResult,
    RequestSource, Selected, SlotChanged, SlotPolicy, SlotRef, Slots, Stacks,
};
use mcrs_minecraft_engine_registries::RegistryNames;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_equipment::{EquipSlot, EquipmentChangeOrigin, EquipmentSlots, diff_equipment};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_inventory_player::{HOTBAR, OFF_HAND};
use mcrs_minecraft_inventory_transaction::Filter;
use mcrs_minecraft_item_stack::{
    Count, Equippable, EquippableSlot, InteractAnimation, NameLookup, NameTable, SoundEventRef,
    SwingKind, write_wire_not_empty,
};
use mcrs_minecraft_item_table::ItemTable;
use mcrs_minecraft_player_holders::PlayerHolders;
use mcrs_minecraft_protocol_game::clientbound::ClientboundSound;
use mcrs_minecraft_protocol_game::item::ProtoStack;
use mcrs_minecraft_protocol_game::{Hand, SwingAnimationKind};
use mcrs_minecraft_registries::{CurrentRegistries, Ref, Wire};
use mcrs_minecraft_replication::protocol_slot;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use mcrs_minecraft_vanilla_inventory::{InventorySet, RequestPurpose, VanillaInventoryRequests};
use mcrs_minecraft_wire::Decode;
use smallvec::SmallVec;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EquipmentSyncSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VanillaEquipmentPlugin;

impl Plugin for VanillaEquipmentPlugin {
    fn build(&self, app: &mut App) {
        let Ok(source) = app.allocate_request_source() else {
            tracing::error!("vanilla equipment could not allocate a request source");
            return;
        };
        app.init_resource::<EquipmentDirtyQueue>()
            .insert_resource(EquipmentRequests {
                source,
                pending: Vec::new(),
            })
            .add_message::<UseItemRequest>()
            .add_message::<SlotChanged>()
            .add_message::<RequestResult>()
            .add_message::<PlayerEquipmentDelta>()
            .add_message::<OutboundPlayerPacket>()
            .add_message::<PendingEquipSound>()
            .add_message::<SwingPlayer>()
            .configure_sets(
                FixedPostUpdate,
                EquipmentSyncSet
                    .in_set(PlayerEquipmentSyncSet)
                    .before(PublishPlayerTilesSet),
            )
            .add_systems(
                FixedUpdate,
                (
                    use_armour_equip
                        .run_if(on_message::<UseItemRequest>)
                        .in_set(TickSet::ClientInput)
                        .after(InventorySet::Input),
                    swing_on_armour_use
                        .run_if(on_message::<RequestResult>)
                        .in_set(TickSet::Simulate)
                        .before(SwingSimulateSet),
                ),
            )
            .add_systems(
                FixedPostUpdate,
                (
                    (
                        (
                            enqueue_added_holders,
                            enqueue_selected_changed,
                            enqueue_slot_changed.run_if(on_message::<SlotChanged>),
                            enqueue_hand_swaps.run_if(on_message::<RequestResult>),
                        )
                            .chain(),
                        sync_equipment
                            .run_if(|queue: Res<EquipmentDirtyQueue>| !queue.dirty.is_empty()),
                    )
                        .chain()
                        .in_set(EquipmentSyncSet),
                    emit_equip_sounds
                        .run_if(on_message::<PendingEquipSound>)
                        .after(EquipmentSyncSet)
                        .after(BlockBreakAckSet),
                ),
            );
    }
}

#[derive(Component, Default)]
#[component(storage = "SparseSet")]
struct OwnArmourEntities([Option<Entity>; 4]);

#[derive(Resource, Default)]
struct EquipmentDirtyQueue {
    dirty: Vec<Entity>,
    added_holders: Vec<Entity>,
    hand_swaps: Vec<Entity>,
    armour_candidates: Vec<(Entity, EquipSlot)>,
}

#[derive(Resource)]
struct EquipmentRequests {
    source: RequestSource,
    pending: Vec<(u32, SwingPlayer)>,
}

#[derive(Message, Clone, Debug)]
struct PendingEquipSound {
    target: PacketTarget,
    sound: ClientboundSound,
}

fn enqueue_added_holders(
    added: Populated<Entity, Added<PlayerHolders>>,
    mut queue: ResMut<EquipmentDirtyQueue>,
) {
    for player in &added {
        queue.dirty.push(player);
        queue.added_holders.push(player);
    }
}

fn enqueue_selected_changed(
    changed: Populated<Entity, (With<PlayerHolders>, Changed<Selected>)>,
    mut queue: ResMut<EquipmentDirtyQueue>,
) {
    queue.dirty.extend(&changed);
}

fn enqueue_slot_changed(
    mut changes: MessageReader<SlotChanged>,
    selected: Query<&Selected, With<PlayerHolders>>,
    mut queue: ResMut<EquipmentDirtyQueue>,
) {
    for change in changes.read() {
        let Ok(sel) = selected.get(change.slot.holder) else {
            continue;
        };
        let Some(slot) = EquipSlot::from_player_slot(change.slot.index, sel.0) else {
            continue;
        };
        queue.dirty.push(change.slot.holder);
        if slot.is_armour() {
            queue.armour_candidates.push((change.slot.holder, slot));
        }
    }
}

fn enqueue_hand_swaps(
    mut results: MessageReader<RequestResult>,
    inv_req: Option<Res<VanillaInventoryRequests>>,
    mut queue: ResMut<EquipmentDirtyQueue>,
) {
    let Some(inv_req) = inv_req else { return };
    let expected = RequestId {
        source: inv_req.source,
        serial: RequestPurpose::HandSwap.serial(),
    };
    for result in results.read() {
        if result.id == expected && result.outcome.is_ok() {
            queue.dirty.push(result.holder);
            queue.hand_swaps.push(result.holder);
        }
    }
}

fn live_armour_entities(slots: &Slots) -> [Option<Entity>; 4] {
    [36, 37, 38, 39].map(|index| slots.get(index))
}

fn apply_creative_armour(
    world: &mut World,
    player: Entity,
    armour_ref: SlotRef,
    spawned: Entity,
    id: RequestId,
) {
    let permit = world.get_entity(player).is_ok_and(|ent| {
        let Some(policy) = ent.get::<SlotPolicy>() else {
            return true;
        };
        let access = policy.access(armour_ref.index);
        let gate = |gate: mcrs_minecraft_engine_item::Gate, stack| {
            let query = GateQuery {
                slot: armour_ref,
                actor: player,
                stack,
            };
            gate.answer(world, &query) == Filter::Permits
        };
        let old = ent
            .get::<Slots>()
            .and_then(|slots| slots.get(usize::from(armour_ref.index)));
        gate(access.place, spawned) && old.is_none_or(|stk| gate(access.take, stk))
    });
    if !permit {
        if world.get_entity(spawned).is_ok() {
            world.despawn(spawned);
        }
        if let Some(mut reqs) = world.get_resource_mut::<EquipmentRequests>() {
            reqs.pending.retain(|(serial, _)| *serial != id.serial);
        }
        return;
    }
    world
        .commands()
        .entity(player)
        .request(Request::new(id).set(armour_ref, Some(spawned), 1));
    world.commands().queue(move |inner: &mut World| {
        let err = inner
            .get_resource::<Messages<RequestResult>>()
            .is_some_and(|res| {
                res.iter_current_update_messages()
                    .any(|entry| entry.holder == player && entry.id == id && entry.outcome.is_err())
            });
        if err && inner.get_entity(spawned).is_ok() {
            inner.despawn(spawned);
        }
    });
    world.flush();
}

fn use_armour_equip(
    mut uses: MessageReader<UseItemRequest>,
    players: Query<
        (
            &Slots,
            &Selected,
            Option<&PlayerGameMode>,
            Option<&Vitals>,
            Has<UnloadedClient>,
        ),
        With<PlayerHolders>,
    >,
    mut stacks: Stacks,
    mut requests: ResMut<EquipmentRequests>,
    mut next_serial: Local<u32>,
    mut commands: Commands,
) {
    for use_req in uses.read() {
        let Ok((slots, selected, game_mode, vitals, unloaded)) = players.get(use_req.entity) else {
            continue;
        };
        if unloaded
            || vitals.is_some_and(|health| !health.is_alive())
            || game_mode.is_some_and(|mode| mode.0 == GameMode::Spectator)
        {
            continue;
        }
        let hand_slot = match use_req.hand {
            Hand::Main if HOTBAR.contains(&u16::from(selected.0)) => u16::from(selected.0),
            Hand::Main => continue,
            Hand::Off => OFF_HAND,
        };
        let Some(hand_ent) = slots.get(usize::from(hand_slot)) else {
            continue;
        };
        let Some(armour) = stacks
            .get::<Equippable>(hand_ent)
            .filter(|equippable| equippable.swappable)
            .and_then(|equippable| match equippable.slot {
                EquippableSlot::Feet => Some(EquipSlot::Feet),
                EquippableSlot::Legs => Some(EquipSlot::Legs),
                EquippableSlot::Chest => Some(EquipSlot::Chest),
                EquippableSlot::Head => Some(EquipSlot::Head),
                _ => None,
            })
        else {
            continue;
        };
        let armour_idx = armour.player_slot(0);
        let Some(hand_stack) = stacks.to_value(hand_ent) else {
            continue;
        };
        let old_arm = slots
            .get(usize::from(armour_idx))
            .and_then(|slot_ent| stacks.to_value(slot_ent));
        if old_arm.as_ref().is_some_and(|old_slot| {
            old_slot.kind == hand_stack.kind && old_slot.patch() == hand_stack.patch()
        }) {
            continue;
        }
        let anim = stacks
            .get::<InteractAnimation>(hand_ent)
            .copied()
            .unwrap_or_default()
            .0;
        let kind = match anim.kind {
            SwingKind::None => SwingAnimationKind::None,
            SwingKind::Whack => SwingAnimationKind::Whack,
            SwingKind::Stab => SwingAnimationKind::Stab,
        };
        let swing = SwingPlayer::new(use_req.entity, use_req.hand, kind, anim.duration.max(0));
        let slot_ref = |index| SlotRef {
            holder: use_req.entity,
            index,
        };
        let (hand_ref, armour_ref) = (slot_ref(hand_slot), slot_ref(armour_idx));
        let serial = *next_serial;
        let id = RequestId {
            source: requests.source,
            serial,
        };
        if game_mode.is_some_and(|mode| mode.0 == GameMode::Creative) {
            let Some(one) = Count::new(1) else {
                continue;
            };
            let mut eq_stack = hand_stack;
            eq_stack.count = one;
            let spawned = commands.spawn_stack(eq_stack).id();
            *next_serial = serial.wrapping_add(1);
            requests.pending.push((serial, swing));
            let actor = use_req.entity;
            commands.queue(move |world: &mut World| {
                apply_creative_armour(world, actor, armour_ref, spawned, id);
            });
            continue;
        }
        let request = if hand_stack.count.get() == 1 {
            Request::new(id).swap(hand_ref, armour_ref)
        } else if old_arm.is_none() {
            Request::new(id).split(hand_ref, armour_ref, 1)
        } else {
            continue;
        };
        *next_serial = serial.wrapping_add(1);
        requests.pending.push((serial, swing));
        commands.entity(use_req.entity).request(request);
    }
}

fn swing_on_armour_use(
    mut results: MessageReader<RequestResult>,
    mut requests: ResMut<EquipmentRequests>,
    mut swings: MessageWriter<SwingPlayer>,
) {
    for result in results.read() {
        if result.id.source != requests.source {
            continue;
        }
        if let Some(pos) = requests
            .pending
            .iter()
            .position(|(pending_serial, _)| *pending_serial == result.id.serial)
        {
            let (_, swing) = requests.pending.swap_remove(pos);
            if result.outcome.is_ok() {
                swings.write(swing);
            }
        }
    }
}

fn resolve_equip_sound(
    sound_ref: Option<&SoundEventRef>,
    names: &RegistryNames<'_>,
    regs: &CurrentRegistries,
) -> Option<(mcrs_minecraft_sound::Sound<Wire>, f64)> {
    match sound_ref? {
        SoundEventRef::Inline(val) => Some((
            Ref::Inline(Arc::new(val.clone())),
            f64::from(val.range.unwrap_or(16.0)),
        )),
        SoundEventRef::Reference(id) => {
            let location = names.name(NameTable::SoundEvent, id.0)?;
            let found = regs.entries::<SoundEvent>()?.by_name(location)?;
            Some((Ref::Id(found.into()), 16.0))
        }
    }
}

fn encode_proto_stack(
    stacks: &mut Stacks,
    slot_ent: Option<Entity>,
    names: &RegistryNames<'_>,
    scratch: &mut Vec<u8>,
) -> Option<ProtoStack<Wire>> {
    let item_stack = stacks.to_value(slot_ent?)?;
    scratch.clear();
    write_wire_not_empty(&item_stack, names, scratch).ok()?;
    let proto = ProtoStack::decode(&mut scratch.as_slice()).ok()?;
    (proto.count > 0).then_some(proto)
}

type EqSlots = EquipmentSlots<ProtoStack<Wire>>;
type ListenerQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Owner, &'static Transform), With<Player>>;

fn resolve_sound_recipients(
    actor: Entity,
    actor_tf: &Transform,
    range: f64,
    listeners: &ListenerQuery<'_, '_>,
) -> SmallVec<[PlayerSession; 8]> {
    let (actor_pos, range_sq) = (actor_tf.translation, range * range);
    let actor_tile = TileKey::at(actor_pos);
    let mut out = SmallVec::new();
    for (viewer_ent, viewer_owner, viewer_tf) in listeners {
        if viewer_ent == actor {
            out.push(viewer_owner.0);
            continue;
        }
        let viewer_pos = viewer_tf.translation;
        let in_view = View {
            center: TileKey::at(viewer_pos),
            radius: VIEW_RADIUS,
            subject: None,
        }
        .contains(actor_tile);
        if in_view && viewer_pos.distance_squared(actor_pos) < range_sq {
            out.push(viewer_owner.0);
        }
    }
    out
}

fn emit_armour_sounds(
    player: Entity,
    eq_state: (&EqSlots, &EqSlots, Option<&[Option<Entity>; 4]>),
    inv: (&Slots, u8),
    candidates: &[(Entity, EquipSlot)],
    listeners: &ListenerQuery<'_, '_>,
    stacks: &mut Stacks,
    out: (
        &RegistryNames<'_>,
        &CurrentRegistries,
        &mut u64,
        &mut MessageWriter<'_, PendingEquipSound>,
    ),
) {
    let Ok((_, _, actor_tf)) = listeners.get(player) else {
        return;
    };
    let ((live, prev, prev_entities), (slots, sel), (names, regs, seq, sounds)) =
        (eq_state, inv, out);
    let mut seen: SmallVec<[EquipSlot; 4]> = SmallVec::new();
    for &(cand_player, slot) in candidates {
        if cand_player != player || seen.contains(&slot) {
            continue;
        }
        seen.push(slot);
        let Some(next) = live.get(slot) else { continue };
        let Some(stack_ent) = slots.get(usize::from(slot.player_slot(sel))) else {
            continue;
        };
        let armour_pos = match slot {
            EquipSlot::Feet => 0,
            EquipSlot::Legs => 1,
            EquipSlot::Chest => 2,
            EquipSlot::Head => 3,
            EquipSlot::MainHand | EquipSlot::OffHand => continue,
        };
        let same_entity = prev_entities
            .and_then(|stored| stored.get(armour_pos).copied().flatten())
            == Some(stack_ent);
        let same_stack = prev
            .get(slot)
            .is_some_and(|old| old.id == next.id && old.components == next.components);
        if same_entity || same_stack {
            continue;
        }
        let Some(eq) = stacks.get::<Equippable>(stack_ent) else {
            continue;
        };
        let Some((sound_event, range)) = resolve_equip_sound(eq.equip_sound.as_ref(), names, regs)
        else {
            continue;
        };
        *seq = seq.wrapping_add(1);
        let mixed = seq.wrapping_add(0x517c_c1b7_2722_0a95)
            ^ player.to_bits()
            ^ u64::from(slot.wire_id()).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let seed = ((mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9) >> 1) as i64;
        let target =
            PacketTarget::PlayerSet(resolve_sound_recipients(player, actor_tf, range, listeners));
        let pos = actor_tf.translation;
        let sound = ClientboundSound {
            sound_event,
            source: ClientboundSound::SOURCE_PLAYERS,
            x: ClientboundSound::eighths(pos.x),
            y: ClientboundSound::eighths(pos.y),
            z: ClientboundSound::eighths(pos.z),
            volume: 1.0,
            pitch: 1.0,
            seed,
        };
        sounds.write(PendingEquipSound { target, sound });
    }
}

#[cfg_attr(
    mcrs_lint,
    expect(
        mcrs::alloc_in_tick,
        reason = "only runs when !queue.dirty.is_empty() and emits wire vectors"
    )
)]
#[expect(clippy::too_many_arguments, reason = "ECS equipment diff system")]
fn sync_equipment(
    mut queue: ResMut<EquipmentDirtyQueue>,
    table: Option<Res<ItemTable>>,
    registries: Option<Res<CurrentRegistries>>,
    mut seed_seq: Local<u64>,
    mut players: Query<
        (
            &Selected,
            &Slots,
            &mut OwnEquipment,
            Option<&mut OwnArmourEntities>,
        ),
        With<PlayerHolders>,
    >,
    listeners: ListenerQuery<'_, '_>,
    mut stacks: Stacks,
    mut deltas: MessageWriter<PlayerEquipmentDelta>,
    mut sounds: MessageWriter<PendingEquipSound>,
    mut commands: Commands,
    mut scratch: Local<Vec<u8>>,
    mut diff_out: Local<Vec<(EquipSlot, Option<ProtoStack<Wire>>)>>,
) {
    let EquipmentDirtyQueue {
        dirty,
        added_holders,
        hand_swaps,
        armour_candidates,
    } = &mut *queue;
    if let (Some(table), Some(registries)) = (table, registries) {
        let names = RegistryNames::new(&registries, &table);
        dirty.sort_unstable();
        dirty.dedup();
        for player in dirty.drain(..) {
            let (just_added, hand_swap) = (
                added_holders.contains(&player),
                hand_swaps.contains(&player),
            );
            let Ok((sel, slots, mut own, mut armour_entities)) = players.get_mut(player) else {
                continue;
            };
            let mut live = EquipmentSlots::new();
            for slot in EquipSlot::ALL {
                let idx = usize::from(slot.player_slot(sel.0));
                live.set(
                    slot,
                    encode_proto_stack(&mut stacks, slots.get(idx), &names, &mut scratch),
                );
            }
            let live_armour = live_armour_entities(slots);
            if let Some(ref mut stored) = armour_entities {
                if !just_added {
                    emit_armour_sounds(
                        player,
                        (&live, &own.0, Some(&stored.0)),
                        (slots, sel.0),
                        armour_candidates,
                        &listeners,
                        &mut stacks,
                        (&names, &registries, &mut seed_seq, &mut sounds),
                    );
                }
                if stored.0 != live_armour {
                    stored.0 = live_armour;
                }
            } else {
                commands
                    .entity(player)
                    .insert(OwnArmourEntities(live_armour));
            }
            let origin = if hand_swap {
                deltas.write(PlayerEquipmentDelta::SwapHands { player });
                EquipmentChangeOrigin::HandSwap
            } else if own.0 != live {
                EquipmentChangeOrigin::SlotUpdate
            } else {
                diff_out.clear();
                continue;
            };
            diff_equipment(&mut own.0, &live, origin, &mut diff_out);
            if !diff_out.is_empty() {
                let slots = diff_out
                    .drain(..)
                    .map(|(slot, stack)| (protocol_slot(slot), stack.unwrap_or_default()))
                    .collect();
                deltas.write(PlayerEquipmentDelta::Slots { player, slots });
            }
        }
    }
    dirty.clear();
    added_holders.clear();
    hand_swaps.clear();
    armour_candidates.clear();
}

fn emit_equip_sounds(
    mut pending: MessageReader<PendingEquipSound>,
    mut out: MessageWriter<OutboundPlayerPacket>,
) {
    for entry in pending.read() {
        out.write(OutboundPlayerPacket::sound(
            entry.target.clone(),
            entry.sound.clone(),
        ));
    }
}
