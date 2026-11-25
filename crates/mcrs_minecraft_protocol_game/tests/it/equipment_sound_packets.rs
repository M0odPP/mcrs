use mcrs_minecraft_item::component::equippable::EquipmentSlot;
use mcrs_minecraft_protocol::{Decode, Encode, Packet, VarInt};
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundEntityEvent, ClientboundSetEquipment, ClientboundSound,
};
use mcrs_minecraft_protocol_game::item::ProtoStack;
use mcrs_minecraft_registries::{Ref, Wire};
use mcrs_minecraft_resource::ResourceLocationBuf;
use mcrs_minecraft_sound::SoundEventValue;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;

fn rt<T: Encode + for<'a> Decode<'a>>(value: &T) -> (T, Vec<u8>) {
    let mut bytes = Vec::new();
    value.encode(&mut bytes).expect("packet must encode");
    let mut slice = bytes.as_slice();
    let decoded = T::decode(&mut slice).expect("packet must decode");
    assert!(
        slice.is_empty(),
        "{} trailing bytes left after decoding packet",
        slice.len()
    );
    (decoded, bytes)
}

#[test]
fn sound_and_swap_event_encode_and_decode_roundtrip() {
    assert_eq!(ClientboundEntityEvent::ID, 34);
    assert_eq!(ClientboundSound::ID, 119);
    assert_eq!(ClientboundEntityEvent::SWAP_HANDS, 55);
    assert_eq!(ClientboundSound::SOURCE_PLAYERS, VarInt(7));
    assert_eq!(
        (
            ClientboundSound::eighths(0.5),
            ClientboundSound::eighths(-60.0)
        ),
        (4, -480)
    );

    let (ev, ev_bytes) = rt(&ClientboundEntityEvent {
        entity_id: 42,
        entity_status: 55,
    });
    assert_eq!((ev.entity_id, ev.entity_status), (42, 55));
    assert_eq!(ev_bytes, [0, 0, 0, 42, 55]);

    let sound_name = b"minecraft:item.armor.equip_gold";
    let loc = ResourceLocationBuf::new("minecraft", "item.armor.equip_gold").expect("loc");
    let val = SoundEventValue {
        sound_id: loc,
        range: Some(16.0),
    };
    let seed: i64 = 7_626_749_080_764_753_471;
    let inline = ClientboundSound {
        sound_event: Ref::Inline(std::sync::Arc::new(val)),
        source: ClientboundSound::SOURCE_PLAYERS,
        x: 4,
        y: -480,
        z: 4,
        volume: 1.0,
        pitch: 1.0,
        seed,
    };
    let (decoded_inline, inline_bytes) = rt(&inline);
    assert_eq!(decoded_inline, inline);

    let mut expected_inline = vec![0, sound_name.len() as u8];
    expected_inline.extend_from_slice(sound_name);
    expected_inline.push(1);
    expected_inline.extend_from_slice(&16.0_f32.to_be_bytes());
    let mut trailer = vec![7];
    trailer.extend_from_slice(&4_i32.to_be_bytes());
    trailer.extend_from_slice(&(-480_i32).to_be_bytes());
    trailer.extend_from_slice(&4_i32.to_be_bytes());
    trailer.extend_from_slice(&1.0_f32.to_be_bytes());
    trailer.extend_from_slice(&1.0_f32.to_be_bytes());
    trailer.extend_from_slice(&seed.to_be_bytes());
    expected_inline.extend_from_slice(&trailer);
    assert_eq!(inline_bytes, expected_inline);

    let sound_id = SoundEvent::ItemArmorEquipIron.id();
    let reg = ClientboundSound {
        sound_event: Ref::<Wire, SoundEvent, SoundEventValue>::Id(sound_id.into()),
        ..inline
    };
    let (decoded_reg, reg_bytes) = rt(&reg);
    assert_eq!(decoded_reg, reg);
    let mut expected_reg = Vec::new();
    VarInt(i32::from(sound_id.number()) + 1)
        .encode(&mut expected_reg)
        .expect("varint encodes");
    expected_reg.extend_from_slice(&trailer);
    assert_eq!(reg_bytes, expected_reg);

    let eq = ClientboundSetEquipment {
        entity_id: VarInt(9),
        slots: vec![
            (EquipmentSlot::MainHand, ProtoStack::default()),
            (EquipmentSlot::Head, ProtoStack::default()),
        ],
    };
    let (decoded_eq, eq_bytes) = rt(&eq);
    assert_eq!(decoded_eq, eq);
    assert_eq!(eq_bytes, [9, 0x80, 0, 5, 0]);
}
