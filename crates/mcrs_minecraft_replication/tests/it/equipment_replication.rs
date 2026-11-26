use mcrs_engine_muxer::{
    EntityEvent, EntityId, Hold, ReplicationCodec, SubjectEvent, Tier, TileDelta, TileKey, ViewerId,
};
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol::packets::table::Decoded;
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundEntityEvent, ClientboundSetEquipment, ClientboundSwingAnimation,
};
use mcrs_minecraft_protocol_game::entity::{EquipmentSlot as Slot, MetaDataValue, MetadataEntry};
use mcrs_minecraft_protocol_game::entity_data_index::LIVING_FLAGS;
use mcrs_minecraft_protocol_game::item::ProtoStack;
use mcrs_minecraft_protocol_game::table::game_clientbound::{ClientboundGamePacket as Pkt, decode};
use mcrs_minecraft_protocol_game::{Hand, SwingAnimationKind};
use mcrs_minecraft_registries::Wire;
use mcrs_minecraft_replication::{
    EntityCodec, PlayerData, PlayerEntries, PlayerSpawn, Pose, SwingRecipients,
};
use mcrs_minecraft_resource::Id;
use mcrs_minecraft_wire::Decode;

const ACTOR: EntityId = EntityId(10);
const OBSERVER: ViewerId = ViewerId(1);
const OBSERVER_B: ViewerId = ViewerId(2);
const GAIN: Hold = Hold::Gain {
    subject: ACTOR,
    tier: Tier::NEAREST,
};

fn stack(raw: u16, count: i32) -> ProtoStack<Wire> {
    ProtoStack {
        count,
        id: Id::checked(raw, 4096).expect("id").into(),
        ..Default::default()
    }
}

fn dec(mut frame: &[u8]) -> Pkt<'_> {
    let id = VarInt::decode(&mut frame).expect("id").0;
    let Decoded::Packet(pkt) = decode(id, &mut frame).expect("pkt") else {
        panic!("expected typed packet")
    };
    assert!(
        frame.is_empty(),
        "{} trailing bytes after decode",
        frame.len()
    );
    pkt
}

fn dec_chunk(mut frame: &[u8]) -> Pkt<'_> {
    let len = usize::try_from(u32::decode(&mut frame).expect("len")).expect("usize");
    assert_eq!(frame.len(), len);
    dec(frame)
}

fn eqs(chunks: &[bytes::Bytes]) -> Vec<Vec<(Slot, ProtoStack<Wire>)>> {
    chunks
        .iter()
        .flat_map(|c| {
            let (mut out, mut r) = (Vec::new(), c.as_ref());
            while !r.is_empty() {
                let len = usize::try_from(u32::decode(&mut r).expect("len")).expect("usize");
                let (frame, rest) = r.split_at(len);
                r = rest;
                if let Pkt::ClientboundSetEquipment(eq) = dec(frame) {
                    out.push(eq.slots);
                }
            }
            out
        })
        .collect()
}

fn send(
    codec: &mut EntityCodec,
    tick: u64,
    items: Vec<EntityEvent<mcrs_minecraft_replication::ReplicationData>>,
) {
    let evs = items
        .into_iter()
        .map(|event| SubjectEvent {
            subject: ACTOR,
            event,
        })
        .collect();
    codec.apply(
        tick,
        &TileDelta {
            tile: TileKey::new(0, 0, 0),
            events: evs,
        },
    );
}

fn spawn_actor(codec: &mut EntityCodec, equipment: Vec<(Slot, ProtoStack<Wire>)>) {
    let spawn = PlayerSpawn {
        viewer: ViewerId(10),
        uuid: uuid::Uuid::from_u128(115),
        username: "Alice".to_owned(),
        properties: Vec::new(),
        pose: Pose {
            position: glam::DVec3::ZERO,
            look: Default::default(),
        },
        game_mode: mcrs_minecraft_game_mode::GameMode::Survival,
        attributes: Vec::new(),
        entries: PlayerEntries::default(),
        equipment,
    };
    send(
        codec,
        0,
        vec![EntityEvent::Spawn(PlayerData::Spawn(spawn).into())],
    );
}

#[test]
fn empty_spawn_emits_no_equipment_and_actor_never_receives_own_updates() {
    let mut codec = EntityCodec::default();
    spawn_actor(&mut codec, Vec::new());

    let (mut obs_spawn, mut self_spawn, mut out) = (Vec::new(), Vec::new(), Vec::new());
    codec.hold(OBSERVER, GAIN, &mut obs_spawn);
    codec.hold(ViewerId(10), GAIN, &mut self_spawn);
    assert!(
        eqs(&obs_spawn).is_empty(),
        "EQP-SYN-001: empty spawn emits zero SetEquipment"
    );
    assert!(
        eqs(&self_spawn).is_empty(),
        "EQP-SYN-003: actor spawn emits zero SetEquipment"
    );

    send(
        &mut codec,
        1,
        vec![EntityEvent::Data(
            PlayerData::Equipment(vec![(Slot::MainHand, stack(20, 1))]).into(),
        )],
    );
    codec.finish_tick(1, &mut out);
    assert!(
        out.iter().all(|vb| vb.viewer != ViewerId(10)),
        "EQP-SYN-003: actor excludes self SetEquipment"
    );
    assert_eq!(out.iter().filter(|vb| vb.viewer == OBSERVER).count(), 1);

    out.clear();
    send(
        &mut codec,
        2,
        vec![EntityEvent::Data(PlayerData::SwapHands.into())],
    );
    codec.finish_tick(2, &mut out);
    assert!(
        out.iter().all(|vb| vb.viewer != ViewerId(10)),
        "EQP-SYN-003: actor excludes self SwapHands"
    );
    assert_eq!(out.iter().filter(|vb| vb.viewer == OBSERVER).count(), 1);
}

#[test]
fn multiple_equipment_updates_in_same_tick_coalesce_and_sort_by_slot_id() {
    let mut codec = EntityCodec::default();
    spawn_actor(&mut codec, Vec::new());
    codec.hold(OBSERVER, GAIN, &mut Vec::new());
    let mut out = Vec::new();
    codec.finish_tick(0, &mut out);
    out.clear();

    send(
        &mut codec,
        1,
        vec![
            EntityEvent::Data(
                PlayerData::Equipment(vec![(Slot::Head, stack(10, 1)), (Slot::Feet, stack(20, 1))])
                    .into(),
            ),
            EntityEvent::Data(
                PlayerData::Equipment(vec![
                    (Slot::Chest, stack(30, 1)),
                    (Slot::Feet, stack(40, 2)),
                ])
                .into(),
            ),
        ],
    );
    codec.finish_tick(1, &mut out);
    let viewer_chunks: Vec<_> = out
        .iter()
        .filter(|vb| vb.viewer == OBSERVER)
        .flat_map(|vb| vb.chunks.iter().cloned())
        .collect();
    assert_eq!(
        eqs(&viewer_chunks),
        [[
            (Slot::Feet, stack(40, 2)),
            (Slot::Chest, stack(30, 1)),
            (Slot::Head, stack(10, 1))
        ]]
    );
}

#[test]
fn spawn_swap_late_join_swing_order_and_shared_bytes() {
    let mut codec = EntityCodec::default();
    spawn_actor(
        &mut codec,
        vec![(Slot::Head, stack(10, 1)), (Slot::MainHand, stack(20, 1))],
    );

    let (mut sa, mut sc, mut out) = (Vec::new(), Vec::new(), Vec::new());
    codec.hold(OBSERVER, GAIN, &mut sa);
    codec.hold(OBSERVER_B, GAIN, &mut Vec::new());
    assert_eq!(
        eqs(&sa),
        [[(Slot::MainHand, stack(20, 1)), (Slot::Head, stack(10, 1))]]
    );
    codec.finish_tick(1, &mut out);
    assert!(
        out.is_empty(),
        "tick after spawn must emit no duplicate equipment: {out:?}"
    );

    send(
        &mut codec,
        2,
        vec![EntityEvent::Data(PlayerData::SwapHands.into())],
    );
    codec.finish_tick(2, &mut out);
    let a: Vec<_> = out.iter().filter(|m| m.viewer == OBSERVER).collect();
    let b: Vec<_> = out.iter().filter(|m| m.viewer == OBSERVER_B).collect();
    assert!(
        a.len() == 1
            && b.len() == 1
            && std::ptr::eq(a[0].chunks[0].as_ptr(), b[0].chunks[0].as_ptr()),
        "observers share swap bytes"
    );
    assert!(
        matches!(dec_chunk(&a[0].chunks[0]), Pkt::ClientboundEntityEvent(e) if e.entity_status == ClientboundEntityEvent::SWAP_HANDS),
        "swap must emit status 55"
    );

    codec.hold(ViewerId(3), GAIN, &mut sc);
    assert_eq!(
        eqs(&sc),
        [[(Slot::OffHand, stack(20, 1)), (Slot::Head, stack(10, 1))]]
    );

    out.clear();
    let slots = vec![
        (Slot::MainHand, stack(30, 1)),
        (Slot::OffHand, ProtoStack::default()),
    ];
    let swing = PlayerData::Swing {
        hand: Hand::Main,
        animation: SwingAnimationKind::Whack,
        duration: 6,
        recipients: SwingRecipients::Observers,
    };
    send(
        &mut codec,
        3,
        vec![
            EntityEvent::Data(PlayerData::Equipment(slots).into()),
            EntityEvent::Data(swing.into()),
        ],
    );
    codec.finish_tick(3, &mut out);
    let a: Vec<_> = out
        .iter()
        .filter(|m| m.viewer == OBSERVER)
        .flat_map(|m| &m.chunks)
        .collect();
    let b: Vec<_> = out
        .iter()
        .filter(|m| m.viewer == OBSERVER_B)
        .flat_map(|m| &m.chunks)
        .collect();
    assert!(
        a.len() == 2 && b.len() == 2 && std::ptr::eq(a[1].as_ptr(), b[1].as_ptr()),
        "observers share SetEquipment bytes"
    );
    assert!(
        matches!(
            dec_chunk(a[0]),
            Pkt::ClientboundSwingAnimation(ClientboundSwingAnimation { .. })
        ),
        "swing precedes SetEquipment"
    );
    assert!(
        matches!(
            dec_chunk(a[1]),
            Pkt::ClientboundSetEquipment(ClientboundSetEquipment { .. })
        ),
        "second packet must be SetEquipment"
    );
}

#[test]
fn same_tick_swap_or_update_with_gain_preserves_order_and_late_join_never_repeats() {
    let mut codec = EntityCodec::default();
    spawn_actor(&mut codec, vec![(Slot::MainHand, stack(20, 1))]);

    let mut obs_spawn = Vec::new();
    codec.hold(OBSERVER, GAIN, &mut obs_spawn);
    assert_eq!(
        eqs(&obs_spawn),
        [[(Slot::MainHand, stack(20, 1))]],
        "EQP-SYN-001: initial viewer spawn sends MainHand item"
    );
    let mut out = Vec::new();
    codec.finish_tick(0, &mut out);
    assert!(
        out.is_empty(),
        "spawn tick with no updates must emit nothing"
    );

    send(
        &mut codec,
        1,
        vec![EntityEvent::Data(PlayerData::SwapHands.into())],
    );
    let mut gain_chunks = Vec::new();
    codec.hold(OBSERVER_B, GAIN, &mut gain_chunks);
    codec.finish_tick(1, &mut out);
    assert_eq!(
        eqs(&gain_chunks),
        [[(Slot::MainHand, stack(20, 1))]],
        "EQP-SYN-005: same-tick Gain sees pre-swap spawn equipment before tick swap"
    );
    let a_tick1: Vec<_> = out.iter().filter(|m| m.viewer == OBSERVER).collect();
    let b_tick1: Vec<_> = out.iter().filter(|m| m.viewer == OBSERVER_B).collect();
    assert!(
        a_tick1.len() == 1
            && b_tick1.len() == 1
            && matches!(
                dec_chunk(&b_tick1[0].chunks[0]),
                Pkt::ClientboundEntityEvent(e) if e.entity_status == ClientboundEntityEvent::SWAP_HANDS
            ),
        "EQP-SYN-005: same-tick Gain viewer receives swap event once matching spawn"
    );

    out.clear();
    let mut late_chunks = Vec::new();
    codec.hold(ViewerId(3), GAIN, &mut late_chunks);
    codec.finish_tick(2, &mut out);
    assert_eq!(
        eqs(&late_chunks),
        [[(Slot::OffHand, stack(20, 1))]],
        "EQP-SYN-001: post-swap late joiner sees swapped OffHand without repeat event"
    );
    assert!(
        out.is_empty(),
        "EQP-SYN-005: existing observers must not receive repeat swap/equipment on late join: {out:?}"
    );

    send(
        &mut codec,
        3,
        vec![EntityEvent::Data(
            PlayerData::Equipment(vec![(Slot::Head, stack(10, 1))]).into(),
        )],
    );
    let mut gain_equip_chunks = Vec::new();
    codec.hold(ViewerId(4), GAIN, &mut gain_equip_chunks);
    codec.finish_tick(3, &mut out);
    assert_eq!(
        eqs(&gain_equip_chunks),
        [[(Slot::OffHand, stack(20, 1))]],
        "EQP-SYN-001: same-tick Gain sees pre-update spawn equipment before finish_tick diff"
    );
    let v4_msgs: Vec<_> = out.iter().filter(|m| m.viewer == ViewerId(4)).collect();
    assert!(
        v4_msgs.len() == 1 && eqs(&v4_msgs[0].chunks) == [[(Slot::Head, stack(10, 1))]],
        "EQP-SYN-002: same-tick Gain viewer gets single incremental SetEquipment delta"
    );
}

#[test]
fn hidden_equipment_updates_and_same_tick_show_with_swap_sync_cleanly() {
    let mut codec = EntityCodec::default();
    spawn_actor(
        &mut codec,
        vec![(Slot::Head, stack(10, 1)), (Slot::OffHand, stack(20, 1))],
    );
    codec.hold(OBSERVER, GAIN, &mut Vec::new());
    let mut out = Vec::new();
    codec.finish_tick(0, &mut out);

    out.clear();
    send(
        &mut codec,
        1,
        vec![EntityEvent::Data(PlayerData::Hide.into())],
    );
    codec.finish_tick(1, &mut out);

    out.clear();
    send(
        &mut codec,
        2,
        vec![EntityEvent::Data(
            PlayerData::Equipment(vec![(Slot::Feet, stack(30, 1))]).into(),
        )],
    );
    codec.finish_tick(2, &mut out);
    assert!(
        out.is_empty(),
        "hidden actor with zero observers emits no equipment packet while updating state"
    );

    send(
        &mut codec,
        3,
        vec![
            EntityEvent::Data(PlayerData::SwapHands.into()),
            EntityEvent::Data(PlayerData::Show.into()),
        ],
    );
    codec.finish_tick(3, &mut out);
    let obs_show: Vec<_> = out
        .iter()
        .filter(|m| m.viewer == OBSERVER)
        .flat_map(|m| m.chunks.iter().cloned())
        .collect();
    assert_eq!(
        eqs(&obs_show),
        [[
            (Slot::OffHand, stack(20, 1)),
            (Slot::Feet, stack(30, 1)),
            (Slot::Head, stack(10, 1))
        ]],
        "EQP-SYN-001: Show carries zero-observer Feet update and pre-swap OffHand"
    );
    assert!(
        obs_show.iter().any(|c| matches!(
            dec_chunk(c),
            Pkt::ClientboundEntityEvent(e) if e.entity_status == ClientboundEntityEvent::SWAP_HANDS
        )),
        "EQP-SYN-005: same-tick SwapHands + Show delivers SWAP_HANDS after Show spawn"
    );
}

#[test]
fn filters_non_player_slots_air_count_and_no_op_slot_updates() {
    let mut codec = EntityCodec::default();
    spawn_actor(
        &mut codec,
        vec![(Slot::MainHand, stack(0, 3)), (Slot::Body, stack(20, 1))],
    );
    let mut spawn_chunks = Vec::new();
    codec.hold(OBSERVER, GAIN, &mut spawn_chunks);
    assert!(
        eqs(&spawn_chunks).is_empty(),
        "EQP-SYN-001 / EQP-SLT-001: air count>0 and Body slot must not populate spawn equipment"
    );
    let mut out = Vec::new();
    codec.finish_tick(0, &mut out);
    out.clear();

    send(
        &mut codec,
        1,
        vec![EntityEvent::Data(
            PlayerData::Equipment(vec![
                (Slot::Body, stack(20, 1)),
                (Slot::Saddle, stack(21, 1)),
            ])
            .into(),
        )],
    );
    codec.finish_tick(1, &mut out);
    assert!(
        out.is_empty(),
        "EQP-SLT-001: Body/Saddle-only update must not emit empty SetEquipment: {out:?}"
    );

    send(
        &mut codec,
        2,
        vec![EntityEvent::Data(
            PlayerData::Equipment(vec![
                (Slot::Body, stack(20, 1)),
                (Slot::Head, stack(10, 1)),
                (Slot::MainHand, stack(0, 2)),
            ])
            .into(),
        )],
    );
    codec.finish_tick(2, &mut out);
    let tick2: Vec<_> = out
        .iter()
        .filter(|m| m.viewer == OBSERVER)
        .flat_map(|m| m.chunks.iter().cloned())
        .collect();
    assert_eq!(
        eqs(&tick2),
        [[(Slot::Head, stack(10, 1))]],
        "EQP-PKT-001: air count>0 on already-empty slot and Body slot are filtered out"
    );

    out.clear();
    send(
        &mut codec,
        3,
        vec![
            EntityEvent::Data(PlayerData::Equipment(vec![(Slot::Head, stack(99, 1))]).into()),
            EntityEvent::Data(PlayerData::Equipment(vec![(Slot::Head, stack(10, 1))]).into()),
        ],
    );
    codec.finish_tick(3, &mut out);
    assert!(
        out.is_empty(),
        "EQP-SYN-002: reverted/unchanged slot within same tick must emit no SetEquipment: {out:?}"
    );

    send(
        &mut codec,
        4,
        vec![EntityEvent::Data(
            PlayerData::Equipment(vec![(Slot::Head, stack(0, 1))]).into(),
        )],
    );
    codec.finish_tick(4, &mut out);
    let tick4: Vec<_> = out
        .iter()
        .filter(|m| m.viewer == OBSERVER)
        .flat_map(|m| m.chunks.iter().cloned())
        .collect();
    assert_eq!(
        eqs(&tick4),
        [[(Slot::Head, ProtoStack::default())]],
        "clearing existing slot with air stack emits wire empty ProtoStack::default()"
    );
}

#[test]
fn living_flags_changes_emit_index_8_metadata_for_observers_and_spawn() {
    let mut prev = PlayerEntries::default();
    for expected in [1_u8, 0, 3, 2, 1] {
        let next = PlayerEntries {
            living_flags: expected,
            ..prev
        };
        assert_ne!(next, prev, "living_flags change must update PartialEq");
        assert_eq!(
            next.changed_from(&prev),
            vec![MetadataEntry {
                index: LIVING_FLAGS,
                value: MetaDataValue::Byte(expected.cast_signed()),
            }],
            "changed_from should emit LIVING_FLAGS index 8 for {expected}"
        );
        prev = next;
    }

    let mut codec = EntityCodec::default();
    spawn_actor(&mut codec, Vec::new());
    codec.hold(OBSERVER, GAIN, &mut Vec::new());
    let mut out = Vec::new();
    codec.finish_tick(0, &mut out);
    out.clear();

    send(
        &mut codec,
        1,
        vec![EntityEvent::Data(
            PlayerData::Entries(PlayerEntries {
                living_flags: 3,
                ..PlayerEntries::default()
            })
            .into(),
        )],
    );
    codec.finish_tick(1, &mut out);
    let viewer_chunks: Vec<_> = out
        .iter()
        .filter(|m| m.viewer == OBSERVER)
        .flat_map(|m| m.chunks.iter().cloned())
        .collect();
    assert!(
        viewer_chunks.iter().any(|chunk| matches!(
            dec_chunk(chunk),
            Pkt::ClientboundSetEntityData(pkt)
                if pkt.metadata.0 == [MetadataEntry {
                    index: LIVING_FLAGS,
                    value: MetaDataValue::Byte(3),
                }]
        )),
        "observer must receive ClientboundSetEntityData with LIVING_FLAGS=3"
    );

    let mut late = Vec::new();
    codec.hold(OBSERVER_B, GAIN, &mut late);
    assert!(
        late.iter().any(|chunk| matches!(
            dec_chunk(chunk),
            Pkt::ClientboundSetEntityData(pkt)
                if pkt.metadata.0.contains(&MetadataEntry {
                    index: LIVING_FLAGS,
                    value: MetaDataValue::Byte(3),
                })
        )),
        "late observer must receive current LIVING_FLAGS on spawn"
    );
}
