use bevy_ecs::entity::Entity;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::prelude::{DetectChangesMut, Local, Mut, Query, Res, ResMut, Single, With};
use glam::DVec3;
use mcrs_engine_muxer::{TileKey, View};
use mcrs_minecraft_damage::{DamageSource, Hit, Vitals};
use mcrs_minecraft_engine::PlayerBody;
use mcrs_minecraft_engine::PlayerMoved;
use mcrs_minecraft_engine::ability::PlayerFlying;
use mcrs_minecraft_engine::bus;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload, PacketTarget};
use mcrs_minecraft_engine::network_id;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine::view_radius::VIEW_RADIUS;
use mcrs_minecraft_engine_damage::{DamageRequest, PendingHits};
use mcrs_minecraft_engine_physics::PhysicsTables;
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::game_rules::GameRules;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::world::dimension::Dimension;
use mcrs_minecraft_engine_world::world::storage::view::{DimensionView, WorldView};
use mcrs_minecraft_entity::mob_effect_keys::MobEffect;
use mcrs_minecraft_entity_attribute::AttributeInstances;
use mcrs_minecraft_entity_attribute::attribute_keys::Attribute;
use mcrs_minecraft_entity_key::{EntityUuid, WriterKey};
use mcrs_minecraft_env_damage as env_damage;
use mcrs_minecraft_env_damage::{
    CONTACT_BOX_SHRINK, FallBlockKind, FallDamageKind, FallResetMode, FallSoundKind, FallStepInput,
    GroundState,
};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_rule::GameRule;
use mcrs_minecraft_physics as physics;
use mcrs_minecraft_physics::{Aabb, BlockFlags, CollisionTable, EFFECT_OFFSET};
use mcrs_minecraft_pose::Dimensions;
use mcrs_minecraft_protocol_game::clientbound::{ClientboundEntityEvent, ClientboundSound};
use mcrs_minecraft_registries::{Ref, Wire};
use mcrs_minecraft_sound::Sound;
use mcrs_minecraft_volume::BlockVolume;
use mcrs_voxel::{BlockPos, BlockStateId};
use smallvec::SmallVec;

use crate::bake;
use crate::bake::{BakedEnvDamage, EnvBlockTable};
use crate::env_state::{EnvTickState, PendingEnvEffect, SoundSpec};

const FALL_SOUND_RANGE: f64 = 16.0;

pub(crate) fn block_state_at(
    blocks: Option<&DimensionView<'_>>,
    block_position: BlockPos,
) -> BlockStateId {
    blocks
        .and_then(|view| view.block(block_position).ok())
        .unwrap_or(BlockStateId::AIR)
}

pub(crate) fn landing_block_position(
    blocks: Option<&DimensionView<'_>>,
    collision: &CollisionTable,
    position: DVec3,
) -> BlockPos {
    let probe = BlockPos::new(
        physics::floor_block(position.x),
        physics::floor_block(position.y - f64::from(EFFECT_OFFSET)),
        physics::floor_block(position.z),
    );
    let below_probe = BlockPos::new(probe.x, probe.y - 1, probe.z);
    let probe_flags = collision.state(block_state_at(blocks, probe)).flags;
    let below_flags = collision.state(block_state_at(blocks, below_probe)).flags;
    let fence_mask = BlockFlags::FENCE | BlockFlags::WALL | BlockFlags::FENCE_GATE;
    if probe_flags.is_empty() && below_flags.intersects(fence_mask) {
        physics::offset_block(Some(below_probe), below_flags, position, EFFECT_OFFSET)
    } else {
        physics::offset_block(None, BlockFlags::empty(), position, EFFECT_OFFSET)
    }
}

pub(crate) fn box_has_block_flag(
    blocks: Option<&DimensionView<'_>>,
    collision: &CollisionTable,
    bounds: Aabb,
    mask: BlockFlags,
) -> bool {
    let (min_x, min_y, min_z) = (
        physics::floor_block(bounds.min.x),
        physics::floor_block(bounds.min.y),
        physics::floor_block(bounds.min.z),
    );
    let (max_x, max_y, max_z) = (
        physics::floor_block(bounds.max.x),
        physics::floor_block(bounds.max.y),
        physics::floor_block(bounds.max.z),
    );
    for block_y in min_y..=max_y {
        for block_z in min_z..=max_z {
            for block_x in min_x..=max_x {
                let inside = bounds.min.x < f64::from(block_x + 1)
                    && bounds.max.x > f64::from(block_x)
                    && bounds.min.y < f64::from(block_y + 1)
                    && bounds.max.y > f64::from(block_y)
                    && bounds.min.z < f64::from(block_z + 1)
                    && bounds.max.z > f64::from(block_z);
                if inside
                    && collision
                        .state(block_state_at(
                            blocks,
                            BlockPos::new(block_x, block_y, block_z),
                        ))
                        .flags
                        .intersects(mask)
                {
                    return true;
                }
            }
        }
    }
    false
}

fn has_fall_immunity_effect(file_state: &PlayerFileState) -> bool {
    file_state.active_effects.iter().any(|effect| {
        effect.id == MobEffect::SlowFalling.as_static_str()
            || effect.id == MobEffect::Levitation.as_static_str()
    })
}

struct FallContext<'a> {
    dimension_view: Option<&'a DimensionView<'a>>,
    collision: &'a CollisionTable,
    baked: &'a BakedEnvDamage,
    block_table: &'a EnvBlockTable,
    now: u64,
    fall_damage_enabled: bool,
}

struct PlayerFallInputs<'a> {
    entity: Entity,
    uuid: EntityUuid,
    position: DVec3,
    body: Dimensions,
    mode: GameMode,
    flying: bool,
    vitals: &'a Vitals,
    attributes: Option<&'a AttributeInstances>,
}

fn process_player_fall(
    context: &FallContext<'_>,
    step: &PlayerMoved,
    player: PlayerFallInputs<'_>,
    file_state: Mut<'_, PlayerFileState>,
    mut tick_state: Mut<'_, EnvTickState>,
    outputs: (
        &mut u32,
        &mut PendingHits,
        &mut MessageWriter<'_, PendingEnvEffect>,
    ),
) {
    let abs_x = step.delta.x.abs();
    let abs_z = step.delta.z.abs();
    if abs_x > tick_state.tick_move_x || abs_z > tick_state.tick_move_z {
        tick_state.set_if_neq(EnvTickState {
            tick_move_x: tick_state.tick_move_x.max(abs_x),
            tick_move_z: tick_state.tick_move_z.max(abs_z),
        });
    }
    if !player.vitals.is_alive() {
        return;
    }

    let silent = file_state.silent;
    let shrunk = Aabb::of_body(player.position, player.body.width, player.body.height)
        .deflate(CONTACT_BOX_SHRINK);
    let reset_mask = BlockFlags::WATER | BlockFlags::FALL_DAMAGE_RESETTING | BlockFlags::CLIMBABLE;
    let reset = if player.mode == GameMode::Spectator
        || player.flying
        || file_state.fall_flying
        || has_fall_immunity_effect(&file_state)
        || box_has_block_flag(
            context.dimension_view,
            context.collision,
            shrunk,
            reset_mask,
        ) {
        FallResetMode::ResetToZero
    } else {
        FallResetMode::Active
    };

    let ground = if step.now_on_ground {
        GroundState::OnGround
    } else {
        GroundState::Airborne
    };
    let outcome = env_damage::step_player_fall(FallStepInput {
        fall_distance: file_state.fall_distance,
        delta_y: step.delta.y,
        ground,
        reset,
    });
    if outcome.next_fall_distance.to_bits() != file_state.fall_distance.to_bits() {
        file_state
            .map_unchanged(|state| &mut state.fall_distance)
            .set_if_neq(outcome.next_fall_distance);
    }
    let Some(landed_distance) = outcome.landed_distance else {
        return;
    };
    if matches!(player.mode, GameMode::Creative | GameMode::Spectator) {
        return;
    }
    apply_landed_fall(context, player, landed_distance, silent, outputs);
}

fn apply_landed_fall(
    context: &FallContext<'_>,
    player: PlayerFallInputs<'_>,
    landed_distance: f64,
    silent: bool,
    (sequence, pending, effects): (
        &mut u32,
        &mut PendingHits,
        &mut MessageWriter<'_, PendingEnvEffect>,
    ),
) {
    let feet_entry = context.block_table.get(block_state_at(
        context.dimension_view,
        physics::block_at(player.position),
    ));
    let landing_position =
        landing_block_position(context.dimension_view, context.collision, player.position);
    let landing_entry = context
        .block_table
        .get(block_state_at(context.dimension_view, landing_position));
    let block_kind = if feet_entry.is_some_and(|entry| entry.fall_kind == FallBlockKind::PowderSnow)
    {
        FallBlockKind::PowderSnow
    } else {
        landing_entry.map_or(FallBlockKind::Default, |entry| entry.fall_kind)
    };

    let safe_fall = player
        .attributes
        .and_then(|instances| instances.value(Attribute::SafeFallDistance))
        .unwrap_or(3.0);
    let fall_multiplier = player
        .attributes
        .and_then(|instances| instances.value(Attribute::FallDamageMultiplier))
        .unwrap_or(1.0);
    let outcome =
        env_damage::compute_fall_outcome(landed_distance, block_kind, safe_fall, fall_multiplier);

    if outcome.honey_slide {
        effects.write(PendingEnvEffect {
            actor: player.entity,
            position: player.position,
            entity_sound: (!silent)
                .then_some(context.baked.honey_slide)
                .flatten()
                .map(|sound_id| bake::sound_spec_wire(sound_id, 1.0, 1.0)),
            block_sound: None,
            honey_slide: true,
        });
    }
    if !context.fall_damage_enabled {
        return;
    }

    if let Some(damage) = outcome.damage {
        let damage_type_id = match outcome.kind {
            FallDamageKind::Fall => context.baked.fall,
            FallDamageKind::Stalagmite => context.baked.stalagmite,
        };
        if let Some(damage_type) = damage_type_id {
            pending.push(DamageRequest::new(
                player.entity,
                Hit::new(damage, DamageSource::of(damage_type)),
                WriterKey::Entity { key: player.uuid },
                context.now,
                *sequence,
            ));
            *sequence = sequence.saturating_add(1);
        }
    }

    if silent || (outcome.damage.is_none() && block_kind != FallBlockKind::PowderSnow) {
        return;
    }
    let entity_sound = match (block_kind, outcome.sound) {
        (FallBlockKind::PowderSnow, FallSoundKind::Small) => context.baked.generic_small_fall,
        (FallBlockKind::PowderSnow, FallSoundKind::Big) => context.baked.generic_big_fall,
        (_, FallSoundKind::Small) => context.baked.player_small_fall,
        (_, FallSoundKind::Big) => context.baked.player_big_fall,
        (_, FallSoundKind::None) => None,
    }
    .map(|sound_id| bake::sound_spec_wire(sound_id, 1.0, 1.0));
    let block_sound = outcome
        .triggers_block_fall_sound
        .then(|| {
            landing_entry.and_then(|entry| {
                entry.fall_sound.as_ref().map(|fall_sound| SoundSpec {
                    sound: fall_sound.sound.clone(),
                    volume: fall_sound.volume,
                    pitch: fall_sound.pitch,
                })
            })
        })
        .flatten();
    if entity_sound.is_some() || block_sound.is_some() {
        effects.write(PendingEnvEffect {
            actor: player.entity,
            position: player.position,
            entity_sound,
            block_sound,
            honey_slide: false,
        });
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "ECS fall collection reads player motion and world tables"
)]
pub(crate) fn collect_fall_damage(
    dimension: Option<Single<Entity, With<Dimension>>>,
    world: WorldView,
    tables: Res<PhysicsTables>,
    baked: Res<BakedEnvDamage>,
    block_table: Res<EnvBlockTable>,
    rules: Option<Res<GameRules>>,
    clock: Option<Res<BirthClock>>,
    mut moved: MessageReader<PlayerMoved>,
    mut pending: ResMut<PendingHits>,
    mut effects: MessageWriter<PendingEnvEffect>,
    mut players: Query<
        (
            &EntityUuid,
            &Transform,
            PlayerBody,
            &PlayerGameMode,
            &PlayerFlying,
            &Vitals,
            Option<&AttributeInstances>,
            &mut PlayerFileState,
            &mut EnvTickState,
        ),
        With<Player>,
    >,
) {
    let dimension_view = dimension.and_then(|dimension_entity| world.get(*dimension_entity));
    let context = FallContext {
        dimension_view: dimension_view.as_ref(),
        collision: tables.collision(),
        baked: &baked,
        block_table: &block_table,
        now: clock.as_deref().map_or(0, |birth| birth.0),
        fall_damage_enabled: rules
            .as_deref()
            .is_none_or(|game_rules| game_rules.is_on(GameRule::FallDamage)),
    };
    let mut sequence = u32::try_from(pending.len()).unwrap_or(u32::MAX);

    for step in moved.read() {
        let Ok((&uuid, transform, body, mode, flying, vitals, attributes, file_state, tick_state)) =
            players.get_mut(step.entity)
        else {
            continue;
        };
        let inputs = PlayerFallInputs {
            entity: step.entity,
            uuid,
            position: transform.translation,
            body: body.dimensions(),
            mode: mode.0,
            flying: flying.0,
            vitals,
            attributes,
        };
        process_player_fall(
            &context,
            step,
            inputs,
            file_state,
            tick_state,
            (&mut sequence, &mut pending, &mut effects),
        );
    }
}

type ListenerQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Owner, &'static Transform), With<Player>>;

fn next_sound_seed(counter: &mut u64, actor: Entity, salt: u64) -> i64 {
    *counter = counter.wrapping_add(1);
    let mixed = counter.wrapping_add(0x517c_c1b7_2722_0a95) ^ actor.to_bits() ^ salt;
    ((mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9) >> 1) as i64
}

fn collect_effect_recipients(
    actor: Entity,
    actor_position: DVec3,
    listeners: &ListenerQuery<'_, '_>,
) -> (SmallVec<[PlayerSession; 8]>, SmallVec<[PlayerSession; 8]>) {
    let range_squared = FALL_SOUND_RANGE * FALL_SOUND_RANGE;
    let actor_tile = TileKey::at(actor_position);
    let mut tracking_recipients = SmallVec::new();
    let mut sound_recipients = SmallVec::new();
    for (viewer_entity, viewer_owner, viewer_transform) in listeners {
        if viewer_entity == actor {
            tracking_recipients.push(viewer_owner.0);
            sound_recipients.push(viewer_owner.0);
            continue;
        }
        let viewer_position = viewer_transform.translation;
        let in_view = View {
            center: TileKey::at(viewer_position),
            radius: VIEW_RADIUS,
            subject: None,
        }
        .contains(actor_tile);
        if !in_view {
            continue;
        }
        tracking_recipients.push(viewer_owner.0);
        if viewer_position.distance_squared(actor_position) < range_squared {
            sound_recipients.push(viewer_owner.0);
        }
    }
    (tracking_recipients, sound_recipients)
}

fn build_sound_packet(
    target: PacketTarget,
    position: DVec3,
    sound_spec: SoundSpec<Sound<Wire>>,
    seed: i64,
) -> OutboundPlayerPacket {
    OutboundPlayerPacket::sound(
        target,
        ClientboundSound {
            sound_event: Ref::clone(&sound_spec.sound),
            source: ClientboundSound::SOURCE_PLAYERS,
            x: ClientboundSound::eighths(position.x),
            y: ClientboundSound::eighths(position.y),
            z: ClientboundSound::eighths(position.z),
            volume: sound_spec.volume,
            pitch: sound_spec.pitch,
            seed,
        },
    )
}

pub(crate) fn emit_env_effects(
    mut pending: MessageReader<PendingEnvEffect>,
    listeners: ListenerQuery<'_, '_>,
    mut seed_sequence: Local<u64>,
    mut packets: MessageWriter<OutboundPlayerPacket>,
) {
    for effect in pending.read() {
        let (tracking_recipients, sound_recipients) =
            collect_effect_recipients(effect.actor, effect.position, &listeners);
        if !sound_recipients.is_empty() {
            if let Some(sound_spec) = effect.entity_sound.clone() {
                let seed = next_sound_seed(&mut seed_sequence, effect.actor, 1);
                packets.write(build_sound_packet(
                    PacketTarget::PlayerSet(sound_recipients.clone()),
                    effect.position,
                    sound_spec,
                    seed,
                ));
            }
            if let Some(sound_spec) = effect.block_sound.clone() {
                let seed = next_sound_seed(&mut seed_sequence, effect.actor, 2);
                packets.write(build_sound_packet(
                    PacketTarget::PlayerSet(sound_recipients),
                    effect.position,
                    sound_spec,
                    seed,
                ));
            }
        }
        if effect.honey_slide
            && !tracking_recipients.is_empty()
            && let Ok((_, owner, _)) = listeners.get(effect.actor)
        {
            packets.write(bus::to_target(
                PacketTarget::PlayerSet(tracking_recipients),
                PacketPayload::EntityEvent(ClientboundEntityEvent {
                    entity_id: network_id::player_network_id(owner.0),
                    entity_status: ClientboundEntityEvent::HONEY_SLIDE,
                }),
            ));
        }
    }
}
