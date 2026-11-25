use std::path::Path;

use bevy_ecs::entity::Entity;
use bevy_math::DVec3;
use mcrs_minecraft_engine::HiddenPlayer;
use mcrs_minecraft_engine::bus::{InboundPlayerDespawn, PacketPayload};
use mcrs_minecraft_engine::channel_types::ToDim;
use mcrs_minecraft_engine::persisted_registry::{PersistedRegistry, PersistedSnapshot};
use mcrs_minecraft_engine_item::{
    HolderCommandsExt, ItemCommandsExt, Request, RequestId, SlotRef, StackCommandsExt,
};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_item::component::equippable::EquipmentSlot as EqSlot;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_item_stack::{Count, Damage, ItemStack, Kind};
use mcrs_minecraft_nbt::compound::NbtCompound;
use mcrs_minecraft_nbt::tag::NbtTag;
use mcrs_minecraft_protocol::packets::table::Decoded;
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol::{Decode, VarInt};
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundEntityEvent, ClientboundSetEquipment, ClientboundSound,
};
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound::{
    ServerboundPlayerAction, ServerboundPlayerLoaded, ServerboundSetCarriedItem, ServerboundUseItem,
};
use mcrs_minecraft_protocol_game::table::game_clientbound::{ClientboundGamePacket, decode};
use mcrs_minecraft_protocol_game::{Direction, Hand};
use mcrs_minecraft_registries::{CurrentRegistries, Ref, Wire};
use mcrs_minecraft_save::PlayerDat;
use mcrs_minecraft_sound::SoundEventValue;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use mcrs_voxel::BlockPos;

use crate::commands::load_stone_column;
use crate::host_app::{self, Arrival, PlayServer, sent_to};

const ALICE: Uuid = Uuid::from_u128(9101);
const BOB: Uuid = Uuid::from_u128(9202);
const CHARLIE: Uuid = Uuid::from_u128(9303);

type DecodedEvents = (
    Vec<&'static str>,
    Vec<ClientboundSetEquipment>,
    Vec<ClientboundSound>,
);

fn start_equipment_server() -> PlayServer {
    let mut app = host_app::make_host_app();
    mcrs_minecraft_engine_registries::install_item_table(
        app.world_mut(),
        mcrs_minecraft_engine_registries::test_item_table(),
    )
    .unwrap();
    app.insert_resource(mcrs_minecraft_command_engine::CommandLimits(
        mcrs_command::Limits::default(),
    ));
    app.add_plugins((
        mcrs_minecraft_environment::WorldClockPlugin,
        mcrs_minecraft_engine::coordinator::clock::WorldClockHostPlugin,
        mcrs_minecraft_engine::coordinator::birth_clock::BirthClockPlugin,
        mcrs_minecraft_command_engine::CommandPlugin::<
            mcrs_minecraft_command_vanilla::VanillaCommands,
        >::new(),
        mcrs_minecraft_server::VanillaEntityPhysicsPlugin,
        mcrs_minecraft_server::VanillaBlocksPlugin,
        mcrs_minecraft_server::VanillaItemEntityPlugin,
        mcrs_minecraft_server::VanillaItemPlugin,
        mcrs_minecraft_server::VanillaBlockPlacePlugin,
        mcrs_minecraft_server::VanillaSwingPlugin,
        mcrs_minecraft_server::VanillaEquipmentPlugin,
        mcrs_minecraft_command_vanilla::VanillaCommandsPlugin,
    ));
    app.insert_resource(mcrs_minecraft_environment::AdvanceTime(false));
    app.insert_resource(bevy_time::TimeUpdateStrategy::FixedTimesteps(1));
    let operator_plugin = mcrs_minecraft_server::operator_broadcast::OperatorBroadcastPlugin;
    mcrs_minecraft_engine_world::world::sub_app::AddDimensionPlugins::add_dimension_plugin(
        &mut app,
        operator_plugin,
    );
    let mut server = PlayServer::start_in(
        app,
        &[("minecraft:overworld", "minecraft:overworld")],
        &[],
        0,
    );
    load_stone_column(&mut server);
    server
}

fn step(server: &mut PlayServer, anchors: &[Entity], ticks: usize) -> Vec<Vec<PacketPayload>> {
    let raw_packets = server.ticks_all(ticks);
    anchors
        .iter()
        .map(|&anchor| sent_to(server.session_of(anchor).unwrap(), raw_packets.clone()))
        .collect()
}

fn push_rep(
    record: &[u8],
    tags: &mut Vec<&'static str>,
    equips: &mut Vec<ClientboundSetEquipment>,
) {
    let mut rest = record;
    while !rest.is_empty() {
        let len = usize::try_from(u32::decode(&mut rest).unwrap()).unwrap();
        let (mut frame, tail) = rest.split_at(len);
        rest = tail;
        let packet_id = VarInt::decode(&mut frame).unwrap().0;
        let Decoded::Packet(packet) = decode(packet_id, &mut frame).unwrap() else {
            continue;
        };
        match packet {
            ClientboundGamePacket::ClientboundSetEquipment(equip) => {
                tags.push("set_equipment");
                equips.push(equip);
            }
            ClientboundGamePacket::ClientboundSwingAnimation(_) => tags.push("swing_animation"),
            ClientboundGamePacket::ClientboundEntityEvent(event)
                if event.entity_status == ClientboundEntityEvent::SWAP_HANDS =>
            {
                tags.push("swap_hands");
            }
            _ => {}
        }
    }
}

fn decode_events(payloads: &[PacketPayload]) -> DecodedEvents {
    let mut tags = Vec::new();
    let mut equips = Vec::new();
    let mut sounds = Vec::new();
    for payload in payloads {
        match payload {
            PacketPayload::Sound(sound) => {
                tags.push("sound");
                sounds.push(sound.clone());
            }
            PacketPayload::Replication(records) => {
                for record in records {
                    push_rep(record, &mut tags, &mut equips);
                }
            }
            _ => {}
        }
    }
    (tags, equips, sounds)
}

fn step_pair(
    server: &mut PlayServer,
    alice: Entity,
    bob: Entity,
) -> (DecodedEvents, DecodedEvents) {
    let packets = step(server, &[alice, bob], 1);
    (decode_events(&packets[0]), decode_events(&packets[1]))
}

fn put(
    server: &mut PlayServer,
    player: Entity,
    slot: u16,
    kind: Item,
    count: u8,
    damage: Option<i32>,
) {
    let mut stack = ItemStack::new(Kind(kind.id().number()), Count::new(count).unwrap());
    if let Some(damage_value) = damage {
        stack = stack.set(Damage::new(damage_value).unwrap());
    }
    let world = server.world();
    world.commands().spawn_stack(stack).put_at(player, slot);
    world.flush();
}

fn saved_stack(kind: Item, count: i32, slot: Option<i8>) -> NbtCompound {
    let mut compound = NbtCompound::new();
    compound.put_string("id", kind.as_static_str().to_owned());
    compound.put_int("count", count);
    if let Some(slot_byte) = slot {
        compound.put_byte("Slot", slot_byte);
    }
    compound
}

fn phase_hand_diffs_and_swap(
    server: &mut PlayServer,
    alice: Entity,
    bob: Entity,
    alice_entity: Entity,
) {
    put(server, alice_entity, 0, Item::DiamondSword, 1, None);
    put(server, alice_entity, 1, Item::IronPickaxe, 1, None);
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        actor.1.is_empty() && actor.2.is_empty(),
        "actor must not receive own SetEquipment or equip sounds: {actor:?}"
    );
    assert!(
        viewer.1.len() == 1
            && viewer.1[0].slots.len() == 1
            && viewer.1[0].slots[0].0 == EqSlot::MainHand
            && viewer.1[0].slots[0].1.id == Item::DiamondSword.id().into()
            && viewer.1[0].slots[0].1.count == 1
            && viewer.2.is_empty(),
        "viewer must receive single MainHand update with zero sounds: {viewer:?}"
    );

    put(server, alice_entity, 9, Item::Emerald, 1, None);
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        actor.0.is_empty() && viewer.0.is_empty(),
        "backpack slot 9 must emit zero SetEquipment or sounds: actor={actor:?}, viewer={viewer:?}"
    );

    put(server, alice_entity, 0, Item::Apple, 1, None);
    let _ = step_pair(server, alice, bob);
    put(server, alice_entity, 0, Item::Apple, 2, None);
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        viewer.1.len() == 1
            && viewer.1[0].slots.len() == 1
            && viewer.1[0].slots[0].1.count == 2
            && actor.2.is_empty()
            && viewer.2.is_empty(),
        "count-only stack change must emit SetEquipment and zero sounds: {viewer:?}"
    );

    put(server, alice_entity, 0, Item::DiamondSword, 1, Some(5));
    let _ = step_pair(server, alice, bob);
    put(server, alice_entity, 0, Item::DiamondSword, 1, Some(10));
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        viewer.1.len() == 1
            && viewer.1[0].slots.len() == 1
            && actor.2.is_empty()
            && viewer.2.is_empty(),
        "damage patch change must emit one SetEquipment and zero sounds: {viewer:?}"
    );

    put(server, alice_entity, 0, Item::DiamondSword, 1, None);
    let _ = step_pair(server, alice, bob);
    server.queue(alice, &ServerboundSetCarriedItem { slot: 1 });
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        viewer.1.len() == 1
            && viewer.1[0].slots.len() == 1
            && viewer.1[0].slots[0].0 == EqSlot::MainHand
            && viewer.1[0].slots[0].1.id == Item::IronPickaxe.id().into()
            && viewer.1[0].slots[0].1.count == 1
            && actor.2.is_empty()
            && viewer.2.is_empty(),
        "hotbar select slot 1 must emit IronPickaxe MainHand and zero sounds: {viewer:?}"
    );
    server.queue(alice, &ServerboundSetCarriedItem { slot: 0 });
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        viewer.1.len() == 1
            && viewer.1[0].slots.len() == 1
            && viewer.1[0].slots[0].0 == EqSlot::MainHand
            && viewer.1[0].slots[0].1.id == Item::DiamondSword.id().into()
            && viewer.1[0].slots[0].1.count == 1
            && actor.2.is_empty()
            && viewer.2.is_empty(),
        "hotbar select slot 0 must emit DiamondSword MainHand and zero sounds: {viewer:?}"
    );

    let swap_action = ServerboundPlayerAction {
        action: PlayerAction::SwapItemWithOffhand,
        pos: BlockPos::new(0, 0, 0),
        direction: Direction::Down,
        sequence: VarInt(1),
    };
    server.queue(alice, &swap_action);
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        viewer.0 == ["swap_hands"] && actor.0.is_empty(),
        "hand swap must emit only entity_event 55 to viewer and nothing to actor: actor={actor:?}, viewer={viewer:?}"
    );
}

fn phase_armour_and_use(server: &mut PlayServer, alice: Entity, bob: Entity, alice_entity: Entity) {
    for (slot, kind) in [
        (36, Item::LeatherBoots),
        (37, Item::ChainmailLeggings),
        (38, Item::DiamondChestplate),
        (39, Item::IronHelmet),
    ] {
        put(server, alice_entity, slot, kind, 1, None);
    }
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        actor.2.len() == 4
            && viewer.2 == actor.2
            && viewer.0 == ["sound", "sound", "sound", "sound", "set_equipment"],
        "armour equip must emit 4 shared sounds to actor and viewer before SetEquipment: actor={:?}, viewer={:?}",
        actor.2,
        viewer.0
    );
    assert!(
        actor.2.iter().all(|sound| {
            sound.source == ClientboundSound::SOURCE_PLAYERS
                && sound.volume == 1.0
                && sound.pitch == 1.0
        }),
        "equip sounds must use source=PLAYERS, volume=1.0, pitch=1.0: {:?}",
        actor.2
    );
    let slot_order: Vec<_> = viewer.1[0]
        .slots
        .iter()
        .map(|(slot_kind, _)| *slot_kind)
        .collect();
    assert_eq!(
        slot_order,
        [EqSlot::Feet, EqSlot::Legs, EqSlot::Chest, EqSlot::Head],
        "viewer must receive equipped armour slots in slot order"
    );

    let source = server
        .world()
        .resource::<mcrs_minecraft_vanilla_inventory::VanillaInventoryRequests>()
        .source;
    let clear_head = Request::new(RequestId { source, serial: 99 }).set(
        SlotRef {
            holder: alice_entity,
            index: 39,
        },
        None,
        0,
    );
    server
        .world()
        .commands()
        .entity(alice_entity)
        .request(clear_head);
    server.world().flush();
    put(server, alice_entity, 0, Item::GoldenHelmet, 1, None);
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        actor.2.is_empty() && viewer.2.is_empty(),
        "clearing armour slot and placing hotbar item must emit zero sounds: actor={:?}, viewer={:?}",
        actor.2,
        viewer.2
    );
    assert!(
        viewer.1.len() == 1
            && viewer.1[0]
                .slots
                .iter()
                .any(|(slot_kind, stack)| *slot_kind == EqSlot::Head && stack.count == 0),
        "clearing armour slot must emit empty Head stack to viewer: {:?}",
        viewer.1
    );
}

fn phase_helmet_use(server: &mut PlayServer, alice: Entity, bob: Entity) {
    server.queue(
        alice,
        &ServerboundUseItem {
            hand: Hand::Main,
            sequence: VarInt(2),
            look: Default::default(),
        },
    );
    let (actor, viewer) = step_pair(server, alice, bob);
    assert!(
        actor.0 == ["sound"] && viewer.0 == ["sound", "swing_animation", "set_equipment"],
        "helmet use must emit sound, swing_animation, then set_equipment: actor={:?}, viewer={:?}",
        actor.0,
        viewer.0
    );
    let registries = server.world().resource::<CurrentRegistries>();
    let gold_id = registries
        .entries::<SoundEvent>()
        .unwrap()
        .by_name(SoundEvent::ItemArmorEquipGold.location())
        .unwrap();
    assert_eq!(
        actor.2[0].sound_event,
        Ref::<Wire, SoundEvent, SoundEventValue>::Id(gold_id.into()),
        "golden helmet equip sound must resolve to item.armor.equip_gold"
    );
    assert!(
        viewer.1[0].slots.len() == 2
            && viewer.1[0].slots[0].0 == EqSlot::MainHand
            && viewer.1[0].slots[0].1.count == 0
            && viewer.1[0].slots[1].0 == EqSlot::Head
            && viewer.1[0].slots[1].1.count == 1,
        "helmet use must clear MainHand and set Head: {:?}",
        viewer.1
    );
}

fn phase_late_join_and_spectator(
    server: &mut PlayServer,
    alice: Entity,
    bob: Entity,
    bob_entity: Entity,
) {
    let (charlie, charlie_join) = server.join(CHARLIE, "Charlie");
    let charlie_settle = step(server, &[charlie], 2);
    let (_, mut charlie_equips, charlie_join_sounds) = decode_events(&charlie_join);
    let (_, charlie_settle_equips, charlie_settle_sounds) = decode_events(&charlie_settle[0]);
    assert!(
        charlie_join_sounds.is_empty() && charlie_settle_sounds.is_empty(),
        "late join must emit zero equip sounds"
    );
    charlie_equips.extend(charlie_settle_equips);
    let combined_slots: Vec<_> = charlie_equips
        .iter()
        .flat_map(|equip| equip.slots.iter().map(|(slot_kind, _)| *slot_kind))
        .collect();
    assert_eq!(
        combined_slots,
        [
            EqSlot::OffHand,
            EqSlot::Feet,
            EqSlot::Legs,
            EqSlot::Chest,
            EqSlot::Head
        ],
        "late joiner must receive actor's non-empty spawn equipment in slot order"
    );
    assert!(
        decode_events(&step(server, &[charlie], 1)[0]).1.is_empty(),
        "charlie must receive no duplicate equipment after spawn"
    );

    server
        .world()
        .get_mut::<PlayerGameMode>(bob_entity)
        .unwrap()
        .0 = GameMode::Spectator;
    put(server, bob_entity, 0, Item::IronHelmet, 1, None);
    let _ = step_pair(server, alice, bob);
    server.queue(
        bob,
        &ServerboundUseItem {
            hand: Hand::Main,
            sequence: VarInt(3),
            look: Default::default(),
        },
    );
    let (alice_after, bob_after) = step_pair(server, alice, bob);
    assert!(
        alice_after.0.is_empty()
            && alice_after.2.is_empty()
            && bob_after.0.is_empty()
            && bob_after.2.is_empty(),
        "spectator item use must emit no equip sound or equipment delta: alice={alice_after:?}, bob={bob_after:?}"
    );
}

#[test]
fn equipment_sync_spawn_updates_swap_sounds_and_late_join() {
    let mut server = start_equipment_server();
    let ((alice, alice_join), (bob, bob_join)) =
        (server.join(ALICE, "Alice"), server.join(BOB, "Bob"));
    let (alice_entity, bob_entity) = (server.player(alice), server.player(bob));
    for player in [alice_entity, bob_entity] {
        server.world().get_mut::<PlayerGameMode>(player).unwrap().0 = GameMode::Survival;
    }
    server.queue(alice, &ServerboundPlayerLoaded);
    server.queue(bob, &ServerboundPlayerLoaded);
    let settle = step(&mut server, &[alice, bob], 5);
    assert!(
        decode_events(&alice_join).2.is_empty()
            && decode_events(&bob_join).2.is_empty()
            && decode_events(&settle[0]).2.is_empty()
            && decode_events(&settle[1]).2.is_empty(),
        "initial joins with empty inventory must emit zero sounds"
    );
    assert!(
        decode_events(&bob_join).1.is_empty() && decode_events(&settle[1]).1.is_empty(),
        "initial empty joins must emit zero SetEquipment packets"
    );
    phase_hand_diffs_and_swap(&mut server, alice, bob, alice_entity);
    phase_armour_and_use(&mut server, alice, bob, alice_entity);
    phase_helmet_use(&mut server, alice, bob);
    phase_late_join_and_spectator(&mut server, alice, bob, bob_entity);
}

#[test]
fn equipment_sync_hidden_player_suppresses_updates_until_shown() {
    let mut server = start_equipment_server();
    let ((alice, _), (bob, _)) = (
        server.join(Uuid::from_u128(9401), "Alice"),
        server.join(Uuid::from_u128(9402), "Bob"),
    );
    let (alice_entity, bob_entity) = (server.player(alice), server.player(bob));
    for player in [alice_entity, bob_entity] {
        server.world().get_mut::<PlayerGameMode>(player).unwrap().0 = GameMode::Survival;
    }
    server.queue(alice, &ServerboundPlayerLoaded);
    server.queue(bob, &ServerboundPlayerLoaded);
    let _ = step(&mut server, &[alice, bob], 5);

    put(&mut server, alice_entity, 0, Item::DiamondSword, 1, None);
    let (_, initial_viewer) = step_pair(&mut server, alice, bob);
    assert_eq!(
        initial_viewer.1.len(),
        1,
        "viewer must see initial weapon before actor is hidden"
    );

    server.world().entity_mut(alice_entity).insert(HiddenPlayer);
    server.world().flush();
    let (_, hidden_viewer) = step_pair(&mut server, alice, bob);
    assert!(
        hidden_viewer.1.is_empty() && hidden_viewer.2.is_empty(),
        "hiding actor must emit zero SetEquipment or equip sounds: {hidden_viewer:?}"
    );

    put(&mut server, alice_entity, 0, Item::IronPickaxe, 1, None);
    put(&mut server, alice_entity, 40, Item::Shield, 1, None);
    let (_, mutated_while_hidden) = step_pair(&mut server, alice, bob);
    assert!(
        mutated_while_hidden.0.is_empty(),
        "changing hidden actor hands must emit zero SetEquipment or sounds: {mutated_while_hidden:?}"
    );

    server.queue(
        alice,
        &ServerboundPlayerAction {
            action: PlayerAction::SwapItemWithOffhand,
            pos: BlockPos::new(0, 0, 0),
            direction: Direction::Down,
            sequence: VarInt(1),
        },
    );
    let (_, swapped_while_hidden) = step_pair(&mut server, alice, bob);
    assert!(
        swapped_while_hidden.0.is_empty(),
        "swapping hands while hidden must emit zero entity events or SetEquipment: {swapped_while_hidden:?}"
    );

    server
        .world()
        .entity_mut(alice_entity)
        .remove::<HiddenPlayer>();
    server.world().flush();
    let shown_packets = step(&mut server, &[alice, bob], 2);
    let (shown_actor, shown_viewer) = (
        decode_events(&shown_packets[0]),
        decode_events(&shown_packets[1]),
    );
    assert!(
        shown_actor.2.is_empty() && shown_viewer.2.is_empty(),
        "showing actor must emit zero equip sounds: actor={:?}, viewer={:?}",
        shown_actor.2,
        shown_viewer.2
    );
    assert!(
        shown_viewer.1.len() == 1 && shown_viewer.1[0].slots.len() == 2,
        "viewer must receive actor's updated equipment bundle upon show: {:?}",
        shown_viewer.1
    );
    assert!(
        shown_viewer.1[0].slots[0].0 == EqSlot::MainHand
            && shown_viewer.1[0].slots[0].1.id == Item::Shield.id().into()
            && shown_viewer.1[0].slots[1].0 == EqSlot::OffHand
            && shown_viewer.1[0].slots[1].1.id == Item::IronPickaxe.id().into(),
        "re-spawned equipment must reflect mutations accumulated while hidden: {:?}",
        shown_viewer.1
    );
    assert!(
        step_pair(&mut server, alice, bob).1.1.is_empty(),
        "re-shown actor must emit zero duplicate SetEquipment on subsequent tick"
    );
}

#[test]
fn equipment_sync_disconnect_cleans_up_and_rejoins_without_stale_state() {
    let mut server = start_equipment_server();
    let alice_uuid = Uuid::from_u128(9501);
    let bob_uuid = Uuid::from_u128(9502);
    let ((alice, _), (bob, _)) = (
        server.join(alice_uuid, "Alice"),
        server.join(bob_uuid, "Bob"),
    );
    let alice_entity = server.player(alice);
    server.queue(alice, &ServerboundPlayerLoaded);
    server.queue(bob, &ServerboundPlayerLoaded);
    let _ = step(&mut server, &[alice, bob], 5);

    put(&mut server, alice_entity, 0, Item::DiamondSword, 1, None);
    put(&mut server, alice_entity, 39, Item::IronHelmet, 1, None);
    let (_, initial_viewer) = step_pair(&mut server, alice, bob);
    assert_eq!(
        initial_viewer.1.len(),
        1,
        "viewer must receive actor's equipment before disconnect"
    );

    let dimension = server.dim;
    let alice_session = server.session_of(alice).unwrap();
    server.submit_to(
        dimension,
        ToDim::Despawn(InboundPlayerDespawn {
            session: alice_session,
        }),
    );
    let despawn_packets = step(&mut server, &[bob], 2);
    let despawn_decoded = decode_events(&despawn_packets[0]);
    assert!(
        despawn_decoded.1.is_empty() && despawn_decoded.2.is_empty(),
        "actor disconnect must emit zero SetEquipment or sounds: {despawn_decoded:?}"
    );
    assert!(
        server.player_in(dimension, alice).is_none(),
        "actor player entity must be removed from dimension after despawn"
    );

    let (alice_rejoined, alice_rejoined_join) = server.join(alice_uuid, "Alice");
    let alice_rejoined_entity = server.player(alice_rejoined);
    server.queue(alice_rejoined, &ServerboundPlayerLoaded);
    let rejoin_packets = step(&mut server, &[alice_rejoined, bob], 2);
    let rejoined_actor = decode_events(&rejoin_packets[0]);
    let rejoined_viewer = decode_events(&rejoin_packets[1]);
    assert!(
        decode_events(&alice_rejoined_join).2.is_empty()
            && rejoined_actor.2.is_empty()
            && rejoined_viewer.1.is_empty()
            && rejoined_viewer.2.is_empty(),
        "empty rejoin after disconnect must leak zero stale SetEquipment or sounds: viewer={rejoined_viewer:?}"
    );

    put(
        &mut server,
        alice_rejoined_entity,
        0,
        Item::GoldenSword,
        1,
        None,
    );
    let (_, after_rejoin_equip) = step_pair(&mut server, alice_rejoined, bob);
    assert!(
        after_rejoin_equip.1.len() == 1
            && after_rejoin_equip.1[0].slots.len() == 1
            && after_rejoin_equip.1[0].slots[0].0 == EqSlot::MainHand
            && after_rejoin_equip.1[0].slots[0].1.id == Item::GoldenSword.id().into()
            && after_rejoin_equip.2.is_empty(),
        "subsequent equipment update on rejoined actor must sync cleanly: {after_rejoin_equip:?}"
    );

    despawn_and_rejoin_viewer(
        &mut server,
        dimension,
        alice_rejoined,
        alice_rejoined_entity,
        bob,
        bob_uuid,
    );
}

fn despawn_and_rejoin_viewer(
    server: &mut PlayServer,
    dimension: Entity,
    alice_rejoined: Entity,
    alice_rejoined_entity: Entity,
    bob: Entity,
    bob_uuid: Uuid,
) {
    let bob_session = server.session_of(bob).unwrap();
    server.submit_to(
        dimension,
        ToDim::Despawn(InboundPlayerDespawn {
            session: bob_session,
        }),
    );
    let _ = step(server, &[alice_rejoined], 2);
    assert!(
        server.player_in(dimension, bob).is_none(),
        "viewer player entity must be removed from dimension after despawn"
    );

    let (bob_rejoined, bob_rejoined_join) = server.join(bob_uuid, "Bob");
    server.queue(bob_rejoined, &ServerboundPlayerLoaded);
    let bob_rejoin_settle = step(server, &[alice_rejoined, bob_rejoined], 2);
    let mut bob_rejoin_equips = decode_events(&bob_rejoined_join).1;
    let (_, bob_settle_equips, bob_settle_sounds) = decode_events(&bob_rejoin_settle[1]);
    assert!(
        decode_events(&bob_rejoined_join).2.is_empty() && bob_settle_sounds.is_empty(),
        "viewer rejoin must emit zero equip sounds"
    );
    bob_rejoin_equips.extend(bob_settle_equips);
    assert!(
        bob_rejoin_equips.len() == 1
            && bob_rejoin_equips[0].slots.len() == 1
            && bob_rejoin_equips[0].slots[0].0 == EqSlot::MainHand
            && bob_rejoin_equips[0].slots[0].1.id == Item::GoldenSword.id().into(),
        "rejoined viewer must receive only current non-empty actor equipment: {bob_rejoin_equips:?}"
    );

    put(server, alice_rejoined_entity, 36, Item::IronBoots, 1, None);
    let (_, post_viewer_rejoin) = step_pair(server, alice_rejoined, bob_rejoined);
    assert!(
        post_viewer_rejoin.1.len() == 1
            && post_viewer_rejoin.1[0].slots.len() == 1
            && post_viewer_rejoin.1[0].slots[0].0 == EqSlot::Feet
            && post_viewer_rejoin.1[0].slots[0].1.id == Item::IronBoots.id().into(),
        "equipment sync after viewer rejoin must deliver new slot delta: {post_viewer_rejoin:?}"
    );
}

#[test]
fn equipment_sync_persisted_snapshot_seeds_spawn_without_sounds_or_duplicate() {
    let mut server = start_equipment_server();
    let (bob, _) = server.join(Uuid::from_u128(9602), "Bob");
    server.queue(bob, &ServerboundPlayerLoaded);
    let _ = step(&mut server, &[bob], 5);

    let mut dat = PlayerDat::default();
    dat.root.put_int("SelectedItemSlot", 2);
    dat.root.put_list(
        "Inventory",
        vec![
            NbtTag::Compound(saved_stack(Item::DiamondSword, 1, Some(2))),
            NbtTag::Compound(saved_stack(Item::Apple, 3, Some(9))),
        ],
    );
    let mut equipment_tag = NbtCompound::new();
    equipment_tag.put("offhand", saved_stack(Item::Shield, 1, None));
    equipment_tag.put("feet", saved_stack(Item::LeatherBoots, 1, None));
    equipment_tag.put("head", saved_stack(Item::IronHelmet, 1, None));
    dat.root.put("equipment", equipment_tag);

    let snapshot = PersistedSnapshot::from_dat(
        &dat,
        server.app.world().resource::<PersistedRegistry>(),
        Path::new("player.dat"),
    )
    .unwrap();
    let arrival = Arrival {
        uuid: Uuid::from_u128(9601),
        name: "Alice",
        position: DVec3::new(8.5, 64.0, 8.5),
        view_distance: 2,
    };
    let alice = host_app::admit_with_file(&mut server.app, server.dim, &arrival, &snapshot);

    let spawn_packets = step(&mut server, &[alice, bob], 3);
    let alice_spawn = decode_events(&spawn_packets[0]);
    let bob_spawn = decode_events(&spawn_packets[1]);
    assert!(
        alice_spawn.1.is_empty() && alice_spawn.2.is_empty() && bob_spawn.2.is_empty(),
        "persisted join must emit zero self SetEquipment and zero equip sounds: alice={alice_spawn:?}, bob={bob_spawn:?}"
    );
    assert_eq!(
        bob_spawn.1.len(),
        1,
        "observer must receive a single spawn SetEquipment bundle for persisted player: {:?}",
        bob_spawn.1
    );
    let expected_slots = [
        (EqSlot::MainHand, Item::DiamondSword),
        (EqSlot::OffHand, Item::Shield),
        (EqSlot::Feet, Item::LeatherBoots),
        (EqSlot::Head, Item::IronHelmet),
    ];
    let observed_slots: Vec<_> = bob_spawn.1[0]
        .slots
        .iter()
        .map(|(slot_kind, stack)| (*slot_kind, stack.id, stack.count))
        .collect();
    let expected_wire: Vec<_> = expected_slots
        .into_iter()
        .map(|(slot_kind, kind)| (slot_kind, kind.id().into(), 1))
        .collect();
    assert_eq!(
        observed_slots, expected_wire,
        "persisted spawn SetEquipment must include only non-empty slots in slot order"
    );

    server.queue(alice, &ServerboundPlayerLoaded);
    let follow_up = step(&mut server, &[alice, bob], 2);
    let alice_follow_up = decode_events(&follow_up[0]);
    let bob_follow_up = decode_events(&follow_up[1]);
    assert!(
        alice_follow_up.2.is_empty() && bob_follow_up.1.is_empty() && bob_follow_up.2.is_empty(),
        "persisted spawn must emit zero tick-1 duplicate SetEquipment or equip sounds: alice={alice_follow_up:?}, bob={bob_follow_up:?}"
    );
}
