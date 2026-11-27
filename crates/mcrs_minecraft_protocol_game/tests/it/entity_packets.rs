use crate::item::harness::{registries, wire_form};
use glam::DVec3;
use mcrs_minecraft_entity::entity_type_keys::EntityType;
use mcrs_minecraft_entity::variant::{CatSoundVariant, CatVariant, ZombieNautilusVariant};
use mcrs_minecraft_entity::villager_profession_keys::VillagerProfession;
use mcrs_minecraft_entity::villager_type_keys::VillagerType;
use mcrs_minecraft_entity_attribute::AttributeModifier;
use mcrs_minecraft_entity_attribute::ModifierOperation;
use mcrs_minecraft_entity_attribute::attribute_keys::Attribute;
use mcrs_minecraft_item::PaintingVariantValue;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_particle::{
    BlockParticle, ColorParticle, DustParticle, ItemParticle, ParticleOptions, ParticleScale,
};
use mcrs_minecraft_pose::Pose;
use mcrs_minecraft_protocol::{Decode, Encode, VarInt};
use mcrs_minecraft_protocol_game::block::BlockStateNumber;
use mcrs_minecraft_protocol_game::clientbound::{
    AttributeSnapshot, ClientboundAddEntity, ClientboundSetEntityData, ClientboundSetEquipment,
    ClientboundSetExperience, ClientboundSetPassengers, ClientboundUpdateAttributes,
};
use mcrs_minecraft_protocol_game::entity::{
    DyeColor, EquipmentSlot, MetaDataValue, Metadata, MetadataEntry, OptionalBlockState,
    OptionalUnsignedInt, VillagerData,
};
use mcrs_minecraft_protocol_game::item::{ArgbInt, ComponentPatch, RgbInt, Template};
use mcrs_minecraft_protocol_game::{ByteAngle, LpVec3, ProtoStack};
use mcrs_minecraft_registries::{Wire, WireId};
use mcrs_minecraft_resource::{Id, ResourceLocation};
use uuid::Uuid;

fn id<R>(number: u16) -> Id<R> {
    Id::checked(number, usize::from(u16::MAX) + 1).unwrap()
}

fn round_trip<'a, P: Encode + Decode<'a> + PartialEq + std::fmt::Debug>(
    packet: &P,
    buf: &'a mut Vec<u8>,
) -> &'a [u8] {
    packet.encode(&mut *buf).expect("encode");
    let mut r: &[u8] = buf;
    let decoded = P::decode(&mut r).expect("decode");
    assert!(r.is_empty(), "{} trailing bytes", r.len());
    assert_eq!(&decoded, packet);
    buf
}

#[test]
fn add_entity_writes_pitch_before_yaw() {
    let packet = ClientboundAddEntity {
        id: VarInt(7),
        uuid: Uuid::nil(),
        kind: EntityType::Player,
        pos: DVec3::ZERO,
        movement: LpVec3(DVec3::ZERO),
        pitch: ByteAngle(1),
        yaw: ByteAngle(2),
        head_yaw: ByteAngle(3),
        data: VarInt(4),
    };
    let mut buf = Vec::new();
    let bytes = round_trip(&packet, &mut buf);
    let tail = &bytes[bytes.len() - 4..];
    assert_eq!(tail, [1, 2, 3, 4]);
}

fn entity_data_entries() -> Vec<(u8, MetaDataValue<'static>, u8)> {
    vec![
        (0, MetaDataValue::Byte(0x20), 0),
        (1, MetaDataValue::VarInt(VarInt(300)), 1),
        (2, MetaDataValue::Float(1.5), 3),
        (3, MetaDataValue::String("mcrs"), 4),
        (4, MetaDataValue::OptionalText(None), 6),
        (
            5,
            MetaDataValue::Slot(wire_form(&ProtoStack::new(
                Item::PhantomMembrane.id(),
                1,
                Default::default(),
            ))),
            7,
        ),
        (6, MetaDataValue::Boolean(true), 8),
        (
            7,
            MetaDataValue::OptionalBlockState(OptionalBlockState(Some(BlockStateNumber(9)))),
            15,
        ),
        (
            8,
            MetaDataValue::Particle(Box::new(ParticleOptions::Dust(DustParticle {
                color: RgbInt(0x123456),
                scale: ParticleScale(2.25),
            }))),
            16,
        ),
        (
            9,
            MetaDataValue::Particles(vec![
                ParticleOptions::AngryVillager,
                ParticleOptions::EntityEffect(ColorParticle {
                    color: ArgbInt(0x80FF8000u32 as i32),
                }),
            ]),
            17,
        ),
        (
            10,
            MetaDataValue::VillagerData(VillagerData::new(
                VillagerType::Plains,
                VillagerProfession::Cleric,
                1,
            )),
            18,
        ),
        (
            11,
            MetaDataValue::OptionalUnsignedInt(OptionalUnsignedInt(Some(0))),
            19,
        ),
        (12, MetaDataValue::Pose(Pose::Sitting), 20),
        (13, MetaDataValue::CatVariant(id::<CatVariant>(1)), 21),
        (
            14,
            MetaDataValue::CatSoundVariant(id::<CatSoundVariant>(0)),
            22,
        ),
        (
            15,
            MetaDataValue::ZombieNautilusVariant(id::<ZombieNautilusVariant>(0)),
            32,
        ),
        (
            16,
            MetaDataValue::PaintingVariant(id::<PaintingVariantValue>(3)),
            34,
        ),
        (17, MetaDataValue::DyeColor(DyeColor::Black), 43),
    ]
}

#[test]
fn entity_data_uses_the_registered_serializer_ids() {
    let entries = entity_data_entries();
    for (index, value, serializer) in &entries {
        let entry = MetadataEntry {
            index: *index,
            value: value.clone(),
        };
        let mut buf = Vec::new();
        entry.encode(&mut buf).unwrap();
        assert_eq!(buf[0], *index);
        assert_eq!(buf[1], *serializer, "{value:?}");
    }

    let packet = ClientboundSetEntityData {
        entity_id: VarInt(7),
        metadata: Metadata(
            entries
                .iter()
                .map(|(index, value, _)| MetadataEntry {
                    index: *index,
                    value: value.clone(),
                })
                .collect(),
        ),
    };
    let mut buf = Vec::new();
    let bytes = round_trip(&packet, &mut buf);
    assert_eq!(*bytes.last().unwrap(), 0xFF);

    let empty = ClientboundSetEntityData {
        entity_id: VarInt(1),
        metadata: Metadata::default(),
    };
    let mut buf = Vec::new();
    assert_eq!(round_trip(&empty, &mut buf), [1, 0xFF]);
}

#[test]
fn optional_wire_forms_reserve_zero_for_absent() {
    let mut buf = Vec::new();
    OptionalBlockState(None).encode(&mut buf).unwrap();
    OptionalUnsignedInt(None).encode(&mut buf).unwrap();
    OptionalUnsignedInt(Some(5)).encode(&mut buf).unwrap();
    assert_eq!(buf, [0, 0, 6]);
    let mut r: &[u8] = &buf;
    assert_eq!(
        OptionalBlockState::decode(&mut r).unwrap(),
        OptionalBlockState(None)
    );
    assert_eq!(
        OptionalUnsignedInt::decode(&mut r).unwrap(),
        OptionalUnsignedInt(None)
    );
    assert_eq!(
        OptionalUnsignedInt::decode(&mut r).unwrap(),
        OptionalUnsignedInt(Some(5))
    );
}

#[test]
fn equipment_chains_slots_with_the_continuation_bit() {
    let packet = ClientboundSetEquipment {
        entity_id: VarInt(9),
        slots: vec![
            (
                EquipmentSlot::MainHand,
                wire_form(&ProtoStack::new(
                    Item::MusicDiscPrecipice.id(),
                    1,
                    Default::default(),
                )),
            ),
            (EquipmentSlot::OffHand, ProtoStack::default()),
            (
                EquipmentSlot::Head,
                wire_form(&ProtoStack::new(Item::Stone.id(), 3, Default::default())),
            ),
        ],
    };
    let mut buf = Vec::new();
    let bytes = round_trip(&packet, &mut buf);
    assert_eq!(
        bytes,
        [9, 0x80, 1, 0xE2, 0x0B, 0, 0, 0x81, 0, 5, 3, 1, 0, 0]
    );

    let single = ClientboundSetEquipment {
        entity_id: VarInt(1),
        slots: vec![(EquipmentSlot::Saddle, ProtoStack::default())],
    };
    let mut buf = Vec::new();
    assert_eq!(round_trip(&single, &mut buf), [1, 7, 0]);

    let mut buf = Vec::new();
    assert!(
        ClientboundSetEquipment {
            entity_id: VarInt(1),
            slots: vec![],
        }
        .encode(&mut buf)
        .is_err()
    );
}

#[test]
fn passengers_round_trip() {
    let packet = ClientboundSetPassengers {
        vehicle: VarInt(12),
        passengers: vec![VarInt(13), VarInt(300)],
    };
    let mut buf = Vec::new();
    assert_eq!(round_trip(&packet, &mut buf), [12, 2, 13, 0xAC, 0x02]);
}

#[test]
fn attributes_round_trip_with_modifiers() {
    let packet = ClientboundUpdateAttributes {
        entity_id: VarInt(5),
        attributes: vec![
            AttributeSnapshot {
                attribute: Attribute::MaxHealth.into(),
                base: 26.0,
                modifiers: vec![],
            },
            AttributeSnapshot {
                attribute: Attribute::KnockbackResistance.into(),
                base: 0.0,
                modifiers: vec![AttributeModifier {
                    id: ResourceLocation::read("minecraft:random_spawn_bonus").unwrap(),
                    amount: 0.025,
                    operation: ModifierOperation::AddValue,
                }],
            },
        ],
    };
    let mut buf = Vec::new();
    let bytes = round_trip(&packet, &mut buf);
    assert_eq!(&bytes[..4], [5, 2, 23, 0x40]);
    assert_eq!(*bytes.last().unwrap(), 0);
}

#[test]
fn a_villager_data_on_the_wire_follows_the_reference_stream_codec() {
    let wire = |kind: i32, profession: i32, level: i32| {
        let mut bytes = Vec::new();
        for number in [kind, profession, level] {
            VarInt(number).encode(&mut bytes).unwrap();
        }
        bytes
    };
    let read = |bytes: Vec<u8>| {
        let mut r = bytes.as_slice();
        let read = VillagerData::decode(&mut r);
        assert!(read.is_err() || r.is_empty(), "{} trailing bytes", r.len());
        read
    };

    let swamp_nitwit = VillagerData::new(VillagerType::Swamp, VillagerProfession::Nitwit, 2);
    assert_eq!(read(wire(5, 11, 2)).unwrap(), swamp_nitwit);
    let mut written = Vec::new();
    swamp_nitwit.encode(&mut written).unwrap();
    assert_eq!(written, wire(5, 11, 2));

    assert_eq!(
        read(wire(2, 0, 0)).unwrap().level(),
        1,
        "a level is at least the first"
    );
    assert_eq!(read(wire(2, 0, -7)).unwrap().level(), 1);
    assert_eq!(
        read(wire(2, 0, 12)).unwrap().level(),
        12,
        "and has no upper bound"
    );
    assert!(read(wire(7, 0, 1)).is_err(), "no seventh villager type");
    assert!(read(wire(2, 15, 1)).is_err(), "no fifteenth profession");
}

#[test]
fn a_player_info_update_reads_the_list_order_and_hat_actions_from_the_top_two_bits() {
    use mcrs_minecraft_protocol::Decode;
    use mcrs_minecraft_protocol_game::clientbound::ClientboundPlayerInfoUpdate;

    let mut frame = vec![0b1100_0000, 1];
    frame.extend_from_slice(&[7; 16]);
    frame.extend_from_slice(&[5, 1]);
    let mut r = &frame[..];
    let update = ClientboundPlayerInfoUpdate::decode(&mut r).unwrap();

    assert!(r.is_empty(), "{} bytes left over", r.len());
    let [entry] = update.entries.as_ref() else {
        panic!("{:?}", update.entries);
    };
    assert_eq!((entry.list_order, entry.show_hat), (5, true));
}

#[test]
fn animate_and_swing_animation_packets_match_wire_layout_and_table_ids() {
    use mcrs_minecraft_protocol::Packet;
    use mcrs_minecraft_protocol_game::clientbound::{
        ClientboundAnimate, ClientboundSwingAnimation,
    };
    use mcrs_minecraft_protocol_game::{Hand, SwingAnimationKind};

    assert_eq!(ClientboundAnimate::ID, 2);
    assert_eq!(ClientboundSwingAnimation::ID, 123);

    for action in [0, 1, 2] {
        let packet = ClientboundAnimate {
            entity_id: VarInt(42),
            action,
        };
        let mut buf = Vec::new();
        let bytes = round_trip(&packet, &mut buf);
        assert_eq!(bytes, [42, action]);
    }

    for (hand, hand_byte) in [(Hand::Main, 0), (Hand::Off, 1)] {
        for (animation, anim_byte) in [
            (SwingAnimationKind::None, 0),
            (SwingAnimationKind::Whack, 1),
            (SwingAnimationKind::Stab, 2),
        ] {
            for (duration, dur_bytes) in [
                (0, &[0u8][..]),
                (6, &[6][..]),
                (17, &[17][..]),
                (150, &[0x96, 0x01][..]),
            ] {
                let packet = ClientboundSwingAnimation {
                    entity_id: VarInt(99),
                    hand,
                    animation,
                    duration: VarInt(duration),
                };
                let mut buf = Vec::new();
                let bytes = round_trip(&packet, &mut buf);
                assert_eq!(&bytes[..3], [99, hand_byte, anim_byte]);
                assert_eq!(&bytes[3..], dur_bytes);
            }
        }
    }
}

#[test]
fn particle_and_particles_entity_data_round_trip_and_check_registries() {
    use mcrs_minecraft_core::codec::Bounded;
    use mcrs_minecraft_protocol_game::item::stack::ItemStackValue;

    let item_particle = wire_form(&ParticleOptions::Item(ItemParticle {
        item: Template::new(Item::DiamondSword.id(), 2, ComponentPatch::EMPTY).unwrap(),
    }));
    let dust_particle = ParticleOptions::<Wire>::Dust(DustParticle {
        color: RgbInt(0x112233),
        scale: ParticleScale(1.5),
    });
    let packet = ClientboundSetEntityData {
        entity_id: VarInt(42),
        metadata: Metadata(vec![
            MetadataEntry {
                index: 10,
                value: MetaDataValue::Particle(Box::new(item_particle.clone())),
            },
            MetadataEntry {
                index: 11,
                value: MetaDataValue::Particles(vec![
                    ParticleOptions::AngryVillager,
                    dust_particle.clone(),
                    item_particle.clone(),
                ]),
            },
            MetadataEntry {
                index: 12,
                value: MetaDataValue::Particles(Vec::new()),
            },
            MetadataEntry {
                index: 13,
                value: MetaDataValue::Boolean(true),
            },
        ]),
    };

    let mut buf = Vec::new();
    round_trip(&packet, &mut buf);
    assert!(
        packet.metadata.check(registries()).is_ok(),
        "valid item particle metadata should check against registries"
    );

    let block_meta = Metadata(vec![MetadataEntry {
        index: 10,
        value: MetaDataValue::Particle(Box::new(ParticleOptions::Block(BlockParticle {
            block_state: BlockStateNumber(1),
        }))),
    }]);
    assert!(
        block_meta.check(registries()).is_ok(),
        "block particle metadata should pass without block definitions"
    );

    let invalid_item = Metadata(vec![MetadataEntry {
        index: 10,
        value: MetaDataValue::Particles(vec![ParticleOptions::Item(ItemParticle {
            item: Template(ItemStackValue {
                item: WireId::new(u16::MAX),
                count: Bounded(1),
                components: ComponentPatch::EMPTY,
            }),
        })]),
    }]);
    assert!(
        invalid_item.check(registries()).is_err(),
        "unknown item id inside particles metadata must be rejected"
    );
}

#[test]
fn set_experience_and_experience_orb_add_entity_match_wire_layout() {
    use mcrs_minecraft_protocol::Packet;

    assert_eq!(ClientboundSetExperience::ID, 105);
    assert_eq!(EntityType::ExperienceOrb.id().number(), 50);

    let packet = ClientboundSetExperience {
        progress: 0.5,
        level: VarInt(3),
        total: VarInt(300),
    };
    let mut buffer = Vec::new();
    let bytes = round_trip(&packet, &mut buffer);
    assert_eq!(bytes, [0x3F, 0x00, 0x00, 0x00, 0x03, 0xAC, 0x02]);

    let add_orb = ClientboundAddEntity {
        id: VarInt(11),
        uuid: Uuid::from_u128(99),
        kind: EntityType::ExperienceOrb,
        pos: DVec3::new(1.5, 64.0, -2.5),
        movement: LpVec3(DVec3::ZERO),
        pitch: ByteAngle(0),
        yaw: ByteAngle(64),
        head_yaw: ByteAngle(0),
        data: VarInt(0),
    };
    let mut orb_buffer = Vec::new();
    let orb_bytes = round_trip(&add_orb, &mut orb_buffer);
    assert_eq!(orb_bytes.get(17).copied(), Some(50));
}

#[test]
fn mob_effect_packets_match_wire_layout_and_table_ids() {
    use mcrs_minecraft_entity::mob_effect_keys::MobEffect;
    use mcrs_minecraft_protocol::Packet;
    use mcrs_minecraft_protocol_game::clientbound::{
        ClientboundRemoveMobEffect, ClientboundUpdateMobEffect,
    };

    assert_eq!(ClientboundRemoveMobEffect::ID, 79);
    assert_eq!(ClientboundUpdateMobEffect::ID, 135);
    let mut bytes = Vec::new();
    let mut update = ClientboundUpdateMobEffect {
        entity_id: VarInt(7),
        effect: MobEffect::Speed,
        amplifier: VarInt(1),
        duration: VarInt(600),
        flags: 0x0E,
    };
    let finite = &[7, 0, 1, 0xD8, 0x04, 0x0E];
    assert_eq!(round_trip(&update, &mut bytes), finite);
    bytes.clear();
    update.duration = VarInt(-1);
    update.flags = ClientboundUpdateMobEffect::AMBIENT | ClientboundUpdateMobEffect::VISIBLE;
    let expected = &[7, 0, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F, 0x03];
    assert_eq!(round_trip(&update, &mut bytes), expected);
    bytes.clear();
    let remove = ClientboundRemoveMobEffect {
        entity_id: VarInt(7),
        effect: MobEffect::Poison,
    };
    assert_eq!(round_trip(&remove, &mut bytes), &[7, 18]);
    let bad = ClientboundRemoveMobEffect::decode(&mut &[7_u8, 41][..]);
    assert!(bad.is_err(), "bad effect id fails decode");
}
