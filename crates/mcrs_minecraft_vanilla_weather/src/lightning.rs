use std::mem::{align_of, size_of};

use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::prelude::{Local, Query, Res, ResMut, With, Without};
use bevy_ecs::system::SystemParam;
use glam::DVec3;
use mcrs_minecraft_block::definition::{BlockStateFlags, Blocks};
use mcrs_minecraft_damage::{DamageSource, Hit, Vitals};
use mcrs_minecraft_dimension_environment::environment::WeatherLevel;
use mcrs_minecraft_engine::bus::{self, OutboundPlayerPacket, PacketPayload, PacketTarget};
use mcrs_minecraft_engine::{ForeignPlayers, PlayerBody};
use mcrs_minecraft_engine_damage::{DamageRequest, PendingHits};
use mcrs_minecraft_engine_entity_motion::{MotionKind, MotionState};
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::block_update::BlockSetRequest;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::entity_ids::CellEntityIds;
use mcrs_minecraft_engine_world::palette::BiomePalette;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::world::dimension::{Dimension, DimensionTypeConfig, HasWeather};
use mcrs_minecraft_engine_world::world::lifecycle::simulation::SectionSimulation;
use mcrs_minecraft_engine_world::world::storage::column::{
    ColumnIndex, ColumnSections, SectionLookup,
};
use mcrs_minecraft_engine_world::world::storage::section::SectionIndex;
use mcrs_minecraft_engine_world::world::sub_app::DimensionLabel;
use mcrs_minecraft_engine_world::world_seed::WorldSeed;
use mcrs_minecraft_entity::entity_type_keys::EntityType;
use mcrs_minecraft_entity_key::{EntityUuid, WriterKey};
use mcrs_minecraft_heightmap::MotionHeightmap;
use mcrs_minecraft_physics::Aabb;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::clientbound::{ClientboundAddEntity, ClientboundSound};
use mcrs_minecraft_protocol_game::{ByteAngle, LpVec3};
use mcrs_minecraft_registries::Ref;
use mcrs_minecraft_simulation::BlockUpdateFlags;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use mcrs_minecraft_weather::{
    LIGHTNING_BASE_DAMAGE, LIGHTNING_CHANCE_DENOMINATOR, LIGHTNING_IMPACT_VOLUME,
    LIGHTNING_ROD_RADIUS, LIGHTNING_STRIKE_BOX_HEIGHT, LIGHTNING_STRIKE_BOX_RADIUS,
    LIGHTNING_THUNDER_VOLUME, WeatherCycleRng,
};
use mcrs_voxel::{BlockPos, BlockStateId, ColumnPos, LocalPos, SectionPos};
use smallvec::SmallVec;
use uuid::Uuid;

use crate::weather_baked::WeatherBaked;

pub(crate) fn thunderstorm_active(
    level: Option<Res<WeatherLevel>>,
    dimensions: Query<(), (With<Dimension>, With<HasWeather>)>,
) -> bool {
    dimensions.single().is_ok()
        && level
            .is_some_and(|level| mcrs_minecraft_weather::is_thunderstorm(level.rain, level.thunder))
}

type ColumnQuery<'w, 's> =
    Query<'w, 's, (&'static ColumnSections, Option<&'static MotionHeightmap>)>;

#[derive(SystemParam)]
pub(crate) struct StrikeWorld<'w, 's> {
    columns: ColumnQuery<'w, 's>,
    section_biomes: Query<'w, 's, &'static BiomePalette>,
    players: Query<
        'w,
        's,
        (
            Entity,
            &'static Transform,
            PlayerBody,
            &'static EntityUuid,
            &'static Owner,
            &'static Vitals,
        ),
        With<Player>,
    >,
    foreign: Option<Res<'w, ForeignPlayers>>,
    label: Option<Res<'w, DimensionLabel>>,
    mobs: Query<
        'w,
        's,
        (
            Entity,
            &'static MotionState,
            &'static MotionKind,
            &'static Vitals,
        ),
        Without<Player>,
    >,
}

#[derive(SystemParam)]
pub(crate) struct StrikeOutputs<'w> {
    entity_ids: ResMut<'w, CellEntityIds>,
    packets: MessageWriter<'w, OutboundPlayerPacket>,
    blocks: MessageWriter<'w, BlockSetRequest>,
    pending_hits: ResMut<'w, PendingHits>,
}

struct StrikeTarget<'a> {
    sec_index: &'a SectionIndex,
    dim: Entity,
    tick: u64,
    pos: BlockPos,
    hit_rod: bool,
}

const _: () = assert!(
    size_of::<StrikeTarget<'static>>() == 40,
    "StrikeTarget size must remain 40 bytes"
);
const _: () = assert!(
    align_of::<StrikeTarget<'static>>() == 8,
    "StrikeTarget alignment must remain 8 bytes"
);

pub(crate) fn strike_lightning(
    dimensions: Query<
        (
            Entity,
            &DimensionTypeConfig,
            &ColumnIndex,
            &SectionIndex,
            &SectionSimulation,
        ),
        (With<Dimension>, With<HasWeather>),
    >,
    world: StrikeWorld,
    blocks: Option<Res<Blocks>>,
    baked: Res<WeatherBaked>,
    timing: (Option<Res<WorldSeed>>, Option<Res<BirthClock>>),
    mut out: StrikeOutputs,
    mut columns_buf: Local<Vec<ColumnPos>>,
) {
    if baked.is_unbaked() {
        return;
    }
    let Ok((dim, config, col_index, sec_index, sim)) = dimensions.single() else {
        return;
    };
    let (seed, tick) = (
        timing.0.map_or(0, |seed| seed.0),
        timing.1.map_or(0, |clock| clock.0),
    );
    let section_size = SectionPos::SIZE as i32;
    let min_sec = config.min_y.div_euclid(section_size);
    let max_sec = min_sec + config.section_count.cast_signed() - 1;
    let max_y =
        (config.min_y + config.section_count.cast_signed() * section_size - 1).max(config.min_y);

    columns_buf.clear();
    for (&col_pos, slot) in &col_index.0 {
        if let Ok((sections, _)) = world.columns.get(slot.entity)
            && (min_sec..=max_sec).any(|sec_y| {
                matches!(sections.lookup(sec_y), SectionLookup::Loaded(_))
                    && sim.is_entity_ticking(SectionPos::new(col_pos.x, sec_y, col_pos.z))
            })
        {
            columns_buf.push(col_pos);
        }
    }
    columns_buf.sort_unstable_by_key(|pos| (pos.x, pos.z));

    for &col_pos in &*columns_buf {
        let mut rng = column_rng(seed, tick, col_pos);
        if !rng.one_in(LIGHTNING_CHANCE_DENOMINATOR) {
            continue;
        }
        let strike_x = col_pos
            .x
            .wrapping_mul(16)
            .wrapping_add(rng.next_u32_bound(16).cast_signed());
        let strike_z = col_pos
            .z
            .wrapping_mul(16)
            .wrapping_add(rng.next_u32_bound(16).cast_signed());
        let Some(initial_y) = height_at(col_index, &world.columns, strike_x, strike_z) else {
            continue;
        };
        let initial = BlockPos::new(strike_x, initial_y, strike_z);
        let (pos, hit_rod) = find_rod(&baked, sec_index, initial, min_sec, max_sec)
            .map_or((initial, false), |rod| {
                (BlockPos::new(rod.x, rod.y + 1, rod.z), true)
            });
        let Some(surface_y) = height_at(col_index, &world.columns, pos.x, pos.z) else {
            continue;
        };
        if pos.y < surface_y {
            continue;
        }
        let biome_pos = BlockPos::new(pos.x, pos.y.clamp(config.min_y, max_y), pos.z);
        let allows_rain = sec_index
            .get(SectionPos::from(biome_pos))
            .and_then(|entity| world.section_biomes.get(entity).ok())
            .is_some_and(|biomes| {
                baked.can_rain(usize::from(biomes.get(LocalPos::from(biome_pos))))
            });
        if allows_rain {
            let target = StrikeTarget {
                sec_index,
                dim,
                tick,
                pos,
                hit_rod,
            };
            execute_strike(
                &target,
                blocks.as_deref(),
                &baked,
                &world,
                &mut out,
                &mut rng,
            );
        }
    }
}

fn column_rng(seed: u64, tick: u64, col: ColumnPos) -> WeatherCycleRng {
    let packed = (u64::from(col.x.cast_unsigned()) << 32) ^ u64::from(col.z.cast_unsigned());
    WeatherCycleRng::new(
        seed ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ packed.wrapping_mul(0xBF58_476D_1CE4_E5B9),
    )
}

fn height_at(
    col_index: &ColumnIndex,
    columns: &ColumnQuery,
    block_x: i32,
    block_z: i32,
) -> Option<i32> {
    let column_pos = ColumnPos::new(block_x.div_euclid(16), block_z.div_euclid(16));
    let (_, heightmap) = columns.get(col_index.0.get(&column_pos)?.entity).ok()?;
    let (local_x, local_z) = (
        block_x.rem_euclid(16) as usize,
        block_z.rem_euclid(16) as usize,
    );
    heightmap.map(|map| map.0.get(local_x, local_z))
}

fn find_rod(
    baked: &WeatherBaked,
    sec_index: &SectionIndex,
    origin: BlockPos,
    min_sec_y: i32,
    max_sec_y: i32,
) -> Option<BlockPos> {
    let radius = LIGHTNING_ROD_RADIUS;
    let mut best: Option<(u64, BlockPos)> = None;
    for sec_y in ((origin.y - radius).div_euclid(16).max(min_sec_y))
        ..=((origin.y + radius).div_euclid(16).min(max_sec_y))
    {
        for sec_z in (origin.z - radius).div_euclid(16)..=(origin.z + radius).div_euclid(16) {
            for sec_x in (origin.x - radius).div_euclid(16)..=(origin.x + radius).div_euclid(16) {
                let Some(blocks) = sec_index.blocks(SectionPos::new(sec_x, sec_y, sec_z)) else {
                    continue;
                };
                let mut any = false;
                blocks
                    .0
                    .for_each_distinct(|state| any |= baked.is_rod(state));
                if !any {
                    continue;
                }
                for local in LocalPos::all() {
                    if !baked.is_rod(blocks.0.get(local)) {
                        continue;
                    }
                    let pos = BlockPos::new(
                        sec_x * 16 + i32::from(local.x()),
                        sec_y * 16 + i32::from(local.y()),
                        sec_z * 16 + i32::from(local.z()),
                    );
                    if let Some(dist_sq) = mcrs_minecraft_weather::within_rod_radius_sq(
                        pos.x - origin.x,
                        pos.y - origin.y,
                        pos.z - origin.z,
                    ) && best.is_none_or(|(best_sq, best_pos)| {
                        (dist_sq, pos.x, pos.y, pos.z)
                            < (best_sq, best_pos.x, best_pos.y, best_pos.z)
                    }) {
                        best = Some((dist_sq, pos));
                    }
                }
            }
        }
    }
    best.map(|(_, pos)| pos)
}

fn strike_recipients(world: &StrikeWorld) -> SmallVec<[PlayerSession; 8]> {
    let mut recipients: SmallVec<[PlayerSession; 8]> = world
        .players
        .iter()
        .map(|(_, _, _, _, owner, _)| owner.0)
        .collect();
    if let (Some(foreign), Some(label)) = (&world.foreign, &world.label) {
        recipients.extend(
            foreign
                .rows()
                .iter()
                .filter(|row| row.dim == label.0)
                .map(|row| row.session),
        );
    }
    recipients
}

fn execute_strike(
    target: &StrikeTarget<'_>,
    blocks: Option<&Blocks>,
    baked: &WeatherBaked,
    world: &StrikeWorld,
    out: &mut StrikeOutputs,
    rng: &mut WeatherCycleRng,
) {
    let strike_pos = DVec3::new(
        f64::from(target.pos.x) + 0.5,
        f64::from(target.pos.y),
        f64::from(target.pos.z) + 0.5,
    );
    emit_strike_packets(strike_pos, world, out, rng);
    if !target.hit_rod
        && let Some(blocks) = blocks
        && baked.fire() != BlockStateId::AIR
    {
        for attempt in 0..5 {
            let pos = if attempt == 0 {
                target.pos
            } else {
                BlockPos::new(
                    target.pos.x + rng.next_u32_bound(3).cast_signed() - 1,
                    target.pos.y + rng.next_u32_bound(3).cast_signed() - 1,
                    target.pos.z + rng.next_u32_bound(3).cast_signed() - 1,
                )
            };
            try_fire(
                target.dim,
                target.sec_index,
                pos,
                blocks,
                baked.fire(),
                &mut out.blocks,
            );
        }
    }
    if let Some(damage_type) = baked.damage_type() {
        apply_strike_damage(
            strike_pos,
            target.tick,
            damage_type,
            world,
            &mut out.pending_hits,
        );
    }
}

fn emit_sound(
    target: PacketTarget,
    sound: SoundEvent,
    strike_pos: DVec3,
    volume: f32,
    pitch: f32,
    seed: i64,
) -> OutboundPlayerPacket {
    OutboundPlayerPacket::sound(
        target,
        ClientboundSound {
            sound_event: Ref::Id(sound.id().into()),
            source: ClientboundSound::SOURCE_WEATHER,
            x: ClientboundSound::eighths(strike_pos.x),
            y: ClientboundSound::eighths(strike_pos.y),
            z: ClientboundSound::eighths(strike_pos.z),
            volume,
            pitch,
            seed,
        },
    )
}

fn emit_strike_packets(
    strike_pos: DVec3,
    world: &StrikeWorld,
    out: &mut StrikeOutputs,
    rng: &mut WeatherCycleRng,
) {
    let recipients = strike_recipients(world);
    if recipients.is_empty() {
        return;
    }
    if let Some(id) = out.entity_ids.allocate()
        && let Ok(wire_id) = i32::try_from(id.0)
    {
        let uuid = Uuid::from_u128((u128::from(rng.next_u64()) << 64) | u128::from(rng.next_u64()));
        out.packets.write(bus::to_target(
            PacketTarget::PlayerSet(recipients.clone()),
            PacketPayload::AddEntity(ClientboundAddEntity {
                id: VarInt(wire_id),
                uuid,
                kind: EntityType::LightningBolt,
                pos: strike_pos,
                movement: LpVec3::default(),
                pitch: ByteAngle::default(),
                yaw: ByteAngle::default(),
                head_yaw: ByteAngle::default(),
                data: VarInt(0),
            }),
        ));
    }
    let pitch_unit = rng.next_u32_bound(1000) as f32 / 1000.0;
    let thunder_seed = rng.next_u64().cast_signed();
    let impact_seed = rng.next_u64().cast_signed();
    out.packets.write(emit_sound(
        PacketTarget::PlayerSet(recipients.clone()),
        SoundEvent::EntityLightningBoltThunder,
        strike_pos,
        LIGHTNING_THUNDER_VOLUME,
        0.8 + pitch_unit * 0.2,
        thunder_seed,
    ));
    out.packets.write(emit_sound(
        PacketTarget::PlayerSet(recipients),
        SoundEvent::EntityLightningBoltImpact,
        strike_pos,
        LIGHTNING_IMPACT_VOLUME,
        0.5 + pitch_unit * 0.2,
        impact_seed,
    ));
}

fn apply_strike_damage(
    strike_pos: DVec3,
    tick: u64,
    damage_type: u32,
    world: &StrikeWorld,
    pending_hits: &mut PendingHits,
) {
    let radius = LIGHTNING_STRIKE_BOX_RADIUS;
    let strike_box = Aabb::new(
        strike_pos - DVec3::splat(radius),
        strike_pos + DVec3::new(radius, LIGHTNING_STRIKE_BOX_HEIGHT + radius, radius),
    );
    let mut seq = u32::try_from(pending_hits.len()).unwrap_or(u32::MAX);
    let hit = Hit::new(LIGHTNING_BASE_DAMAGE, DamageSource::of(damage_type));
    let mut apply = |entity, aabb: Aabb, uuid, vitals: Vitals| {
        if vitals.is_alive() && aabb.intersects(&strike_box) {
            let writer = WriterKey::Entity { key: uuid };
            pending_hits.push(DamageRequest::new(entity, hit, writer, tick, seq));
            seq = seq.saturating_add(1);
        }
    };
    for (entity, transform, body, uuid, _, vitals) in &world.players {
        let body_dims = body.dimensions();
        let aabb = Aabb::of_body(transform.translation, body_dims.width, body_dims.height);
        apply(entity, aabb, *uuid, *vitals);
    }
    for (entity, state, kind, vitals) in &world.mobs {
        let body_dims = kind.0.dimensions;
        let aabb = Aabb::of_body(state.0.body.position, body_dims.width, body_dims.height);
        apply(entity, aabb, state.0.key, *vitals);
    }
}

fn try_fire(
    dim: Entity,
    sec_index: &SectionIndex,
    pos: BlockPos,
    blocks: &Blocks,
    fire: BlockStateId,
    out: &mut MessageWriter<BlockSetRequest>,
) {
    let is_air = sec_index
        .blocks(SectionPos::from(pos))
        .is_some_and(|section| {
            let state = blocks.state(section.0.get(LocalPos::from(pos)));
            state.flags.contains(BlockStateFlags::IS_AIR)
        });
    let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
    let solid_below = sec_index
        .blocks(SectionPos::from(below))
        .is_some_and(|section| {
            let block_state = blocks.state(section.0.get(LocalPos::from(below)));
            !block_state.flags.contains(BlockStateFlags::IS_AIR) && block_state.fluid.is_none()
        });
    if is_air && solid_below {
        out.write(BlockSetRequest {
            dimension: dim,
            pos,
            new_state: fire,
            flags: BlockUpdateFlags::all(),
            recursion_left: 512,
        });
    }
}
