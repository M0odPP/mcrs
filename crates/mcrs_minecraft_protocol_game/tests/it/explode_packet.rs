use glam::DVec3;
use mcrs_minecraft_particle::ParticleOptions;
use mcrs_minecraft_protocol::packets::table::Decoded;
use mcrs_minecraft_protocol::{Decode, Encode, Packet, VarInt};
use mcrs_minecraft_protocol_game::clientbound::{ClientboundExplode, ExplosionParticleEntry};
use mcrs_minecraft_protocol_game::table::game_clientbound;
use mcrs_minecraft_registries::Ref;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;

fn assert_wire(expected: &ClientboundExplode, hex: &str) -> Vec<u8> {
    let wire: Vec<u8> = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("hex"), 16).expect("byte"))
        .collect();
    let (mut reader, mut table_reader) = (wire.as_slice(), wire.as_slice());
    let decoded = ClientboundExplode::decode(&mut reader).expect("decode");
    assert!(
        decoded == *expected && reader.is_empty(),
        "direct decode mismatch"
    );
    let mut reencoded = Vec::new();
    decoded.encode(&mut reencoded).expect("encode");
    assert_eq!(reencoded, wire);
    let table_decoded =
        game_clientbound::decode(ClientboundExplode::ID, &mut table_reader).expect("table");
    assert!(
        table_reader.is_empty()
            && matches!(table_decoded, Decoded::Packet(game_clientbound::ClientboundGamePacket::ClientboundExplode(packet)) if packet == *expected),
        "table must decode id 36 as ClientboundExplode"
    );
    wire
}

#[test]
fn explode_packet_id_and_wire_vectors() {
    use ParticleOptions::{Explosion, ExplosionEmitter, GustEmitterSmall, Poof, Smoke};
    use SoundEvent::{EntityGenericExplode, EntityWindChargeWindBurst};
    assert_eq!(ClientboundExplode::ID, 36);
    let entry = |particle, scaling| ExplosionParticleEntry {
        particle,
        scaling,
        speed: 1.0,
        weight: VarInt(1),
    };
    let sound = |event: SoundEvent| Some(Ref::Id(event.id().into()));
    let y_tnt = -60.0 + f64::from(0.98_f32) * 0.0625;
    let particles = vec![entry(Poof, 0.5), entry(Smoke, 1.0)];
    let knockback = Some(DVec3::new(
        f64::from_bits(0xbfe2_ed44_ef96_4111),
        f64::from_bits(0x3fdd_8090_421c_9ad6),
        0.0,
    ));
    let tnt = ClientboundExplode {
        center: DVec3::new(1.5, y_tnt, 2.5),
        radius: 4.0,
        block_count: 684,
        player_knockback: knockback,
        explosion_particle: Some(ExplosionEmitter),
        explosion_sound: sound(EntityGenericExplode),
        block_particles: particles.clone(),
    };
    let tnt_bytes = assert_wire(
        &tnt,
        "3ff8000000000000c04df828f5c00000400400000000000040800000000002ac01bfe2ed44ef9641113fdd8090421c9ad60000000000000000011d01f70302453f0000003f80000001483f8000003f80000001",
    );
    let silent = ClientboundExplode {
        center: DVec3::new(0.5, y_tnt, 10.5),
        radius: 1.0,
        block_count: 27,
        player_knockback: None,
        explosion_particle: Some(Explosion),
        explosion_sound: None,
        block_particles: particles,
    };
    assert_wire(
        &silent,
        "3fe0000000000000c04df828f5c0000040250000000000003f8000000000001b00011e0002453f0000003f80000001483f8000003f80000001",
    );
    let wind = ClientboundExplode {
        center: DVec3::new(-0.5, -62.75, 2.5),
        radius: 1.2,
        block_count: 35,
        explosion_particle: Some(GustEmitterSmall),
        explosion_sound: sound(EntityWindChargeWindBurst),
        block_particles: Vec::new(),
        ..silent
    };
    assert_wire(
        &wind,
        "bfe0000000000000c04f60000000000040040000000000003f99999a0000002300012301cd0900",
    );
    let mut truncated = tnt_bytes.split_last().expect("non-empty").1;
    assert!(
        ClientboundExplode::decode(&mut truncated).is_err(),
        "truncated explode"
    );
}
