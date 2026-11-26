use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{DetectChangesMut, Mut, Populated, Query, Res, ResMut, With};
use glam::DVec3;
use mcrs_minecraft_damage::{DamageSource, Hit, Vitals};
use mcrs_minecraft_engine as engine;
use mcrs_minecraft_engine::client_on_ground::ClientOnGround;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine::{PlayerBody, ShiftKey};
use mcrs_minecraft_engine_damage::{DamageRequest, PendingHits};
use mcrs_minecraft_engine_physics::PhysicsTables;
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::game_rules::GameRules;
use mcrs_minecraft_engine_world::world::dimension::{Dimension, DimensionTypeConfig};
use mcrs_minecraft_engine_world::world::storage::view::{DimensionView, WorldView};
use mcrs_minecraft_engine_world::world_border::DimensionWorldBorder;
use mcrs_minecraft_entity_key::{EntityUuid, WriterKey};
use mcrs_minecraft_env_damage as env_damage;
use mcrs_minecraft_env_damage::{
    AxisAlignedBounds, CACTUS_DAMAGE, CAMPFIRE_DAMAGE, CONTACT_BOX_SHRINK, ContactHazard,
    IN_WALL_DAMAGE, SOUL_CAMPFIRE_DAMAGE, SuffocationState, WorldBorderBounds,
    WorldBorderDamageConfig,
};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_rule::GameRule;
use mcrs_minecraft_physics as physics;
use mcrs_minecraft_physics::{Aabb, BlockFlags, Collider, CollisionTable, ShapeClass};
use mcrs_minecraft_pose::Dimensions;
use mcrs_voxel::BlockPos;

use crate::bake::{BakedEnvDamage, EnvBlockTable, EnvContactBlock};
use crate::env_state::EnvTickState;
use crate::fall_system;

fn push_env_hit(
    pending: &mut PendingHits,
    target: Entity,
    uuid: EntityUuid,
    tick: u64,
    sequence: &mut u32,
    damage_type: Option<u32>,
    amount: f32,
) {
    let Some(type_id) = damage_type else {
        return;
    };
    pending.push(DamageRequest::new(
        target,
        Hit::new(amount, DamageSource::of(type_id)),
        WriterKey::Entity { key: uuid },
        tick,
        *sequence,
    ));
    *sequence = sequence.saturating_add(1);
}

fn is_suffocating(
    blocks: Option<&DimensionView<'_>>,
    collision: &CollisionTable,
    eye: DVec3,
    width: f32,
) -> bool {
    let eye_box = env_damage::suffocation_eye_box(eye.x, eye.y, eye.z, width);
    let eye_aabb = Aabb::new(
        DVec3::new(eye_box.min_x, eye_box.min_y, eye_box.min_z),
        DVec3::new(eye_box.max_x, eye_box.max_y, eye_box.max_z),
    );
    let (min_x, min_y, min_z) = (
        physics::floor_block(eye_box.min_x),
        physics::floor_block(eye_box.min_y),
        physics::floor_block(eye_box.min_z),
    );
    let (max_x, max_y, max_z) = (
        physics::floor_block(eye_box.max_x),
        physics::floor_block(eye_box.max_y),
        physics::floor_block(eye_box.max_z),
    );
    for block_y in min_y..=max_y {
        for block_z in min_z..=max_z {
            for block_x in min_x..=max_x {
                let block_position = BlockPos::new(block_x, block_y, block_z);
                let data = collision.state(fall_system::block_state_at(blocks, block_position));
                if !data.flags.contains(BlockFlags::SUFFOCATES) {
                    continue;
                }
                let shape = match data.class {
                    ShapeClass::FullCube => collision.full_cube(),
                    ShapeClass::OneBox(id) | ShapeClass::Compound(id) => id,
                    ShapeClass::Scaffolding { .. } => collision.scaffolding_top(),
                    ShapeClass::Empty | ShapeClass::PowderSnow => continue,
                };
                let collider = Collider::Block {
                    shape,
                    at: block_position,
                };
                if collider.intersects(collision, &eye_aabb) {
                    return true;
                }
            }
        }
    }
    false
}

struct HazardContext<'a> {
    dimension_view: Option<&'a DimensionView<'a>>,
    collision: &'a CollisionTable,
    baked: &'a BakedEnvDamage,
    block_table: &'a EnvBlockTable,
    min_y: i32,
    now: u64,
    fire_damage_enabled: bool,
    border_bounds: WorldBorderBounds,
    border_config: WorldBorderDamageConfig,
}

struct PlayerHazardInputs {
    player: Entity,
    uuid: EntityUuid,
    position: DVec3,
    body: Dimensions,
    mode: GameMode,
    on_ground: bool,
    shift: bool,
}

fn collect_contact_hazards(
    context: &HazardContext<'_>,
    inputs: &PlayerHazardInputs,
    bounds: Aabb,
    tick_state: &EnvTickState,
    sequence: &mut u32,
    pending: &mut PendingHits,
) {
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
                if !inside {
                    continue;
                }
                let Some(entry) = context.block_table.get(fall_system::block_state_at(
                    context.dimension_view,
                    BlockPos::new(block_x, block_y, block_z),
                )) else {
                    continue;
                };
                match entry.contact {
                    EnvContactBlock::Cactus => {
                        push_env_hit(
                            pending,
                            inputs.player,
                            inputs.uuid,
                            context.now,
                            sequence,
                            context.baked.cactus,
                            CACTUS_DAMAGE,
                        );
                    }
                    EnvContactBlock::CampfireLit if context.fire_damage_enabled => {
                        push_env_hit(
                            pending,
                            inputs.player,
                            inputs.uuid,
                            context.now,
                            sequence,
                            context.baked.campfire,
                            CAMPFIRE_DAMAGE,
                        );
                    }
                    EnvContactBlock::SoulCampfireLit if context.fire_damage_enabled => {
                        push_env_hit(
                            pending,
                            inputs.player,
                            inputs.uuid,
                            context.now,
                            sequence,
                            context.baked.campfire,
                            SOUL_CAMPFIRE_DAMAGE,
                        );
                    }
                    EnvContactBlock::SweetBerryBush(age) => {
                        let hazard = ContactHazard::SweetBerryBush {
                            age,
                            delta_x: tick_state.tick_move_x,
                            delta_z: tick_state.tick_move_z,
                        };
                        if let Some(damage) = env_damage::contact_damage(hazard) {
                            push_env_hit(
                                pending,
                                inputs.player,
                                inputs.uuid,
                                context.now,
                                sequence,
                                context.baked.sweet_berry_bush,
                                damage,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn check_hot_floor(
    context: &HazardContext<'_>,
    inputs: &PlayerHazardInputs,
    sequence: &mut u32,
    pending: &mut PendingHits,
) {
    if !inputs.on_ground || !context.fire_damage_enabled {
        return;
    }
    let step_position = fall_system::landing_block_position(
        context.dimension_view,
        context.collision,
        inputs.position,
    );
    let is_magma = context
        .block_table
        .get(fall_system::block_state_at(
            context.dimension_view,
            step_position,
        ))
        .is_some_and(|entry| entry.contact == EnvContactBlock::MagmaBlock);
    if !is_magma {
        return;
    }
    let hazard = if inputs.shift {
        ContactHazard::MagmaCrouching
    } else {
        ContactHazard::MagmaStanding
    };
    if let Some(damage) = env_damage::contact_damage(hazard) {
        push_env_hit(
            pending,
            inputs.player,
            inputs.uuid,
            context.now,
            sequence,
            context.baked.hot_floor,
            damage,
        );
    }
}

fn evaluate_player_hazards(
    context: &HazardContext<'_>,
    inputs: PlayerHazardInputs,
    file_state: Mut<'_, PlayerFileState>,
    tick_state: &EnvTickState,
    sequence: &mut u32,
    pending: &mut PendingHits,
) {
    let sleeping = file_state.sleeping_pos.is_some();
    let shrunk = Aabb::of_body(inputs.position, inputs.body.width, inputs.body.height)
        .deflate(CONTACT_BOX_SHRINK);
    let spectator = inputs.mode == GameMode::Spectator;

    if !spectator
        && file_state.fall_distance > 0.0
        && fall_system::box_has_block_flag(
            context.dimension_view,
            context.collision,
            shrunk,
            BlockFlags::WATER,
        )
    {
        file_state
            .map_unchanged(|state| &mut state.fall_distance)
            .set_if_neq(0.0);
    }

    if let Some(damage) = env_damage::void_damage(inputs.position.y, context.min_y) {
        push_env_hit(
            pending,
            inputs.player,
            inputs.uuid,
            context.now,
            sequence,
            context.baked.out_of_world,
            damage,
        );
    }

    if spectator {
        return;
    }

    let suffocation = if !sleeping
        && is_suffocating(
            context.dimension_view,
            context.collision,
            engine::eye_position(inputs.position, inputs.body),
            inputs.body.width,
        ) {
        push_env_hit(
            pending,
            inputs.player,
            inputs.uuid,
            context.now,
            sequence,
            context.baked.in_wall,
            IN_WALL_DAMAGE,
        );
        SuffocationState::InsideWall
    } else {
        SuffocationState::Clear
    };

    if suffocation == SuffocationState::Clear {
        let body = AxisAlignedBounds::from_feet(
            inputs.position.x,
            inputs.position.y,
            inputs.position.z,
            inputs.body.width,
            inputs.body.height,
        );
        if let Some(damage) = env_damage::border_damage(
            inputs.position.x,
            inputs.position.z,
            body,
            context.border_bounds,
            context.border_config,
            suffocation,
        ) {
            push_env_hit(
                pending,
                inputs.player,
                inputs.uuid,
                context.now,
                sequence,
                context.baked.outside_border,
                damage,
            );
        }
    }

    check_hot_floor(context, &inputs, sequence, pending);
    collect_contact_hazards(context, &inputs, shrunk, tick_state, sequence, pending);
}

#[expect(
    clippy::too_many_arguments,
    reason = "Per-tick env hazards inspect player state, world, border, and rules"
)]
pub(crate) fn collect_env_hazards(
    dimensions: Query<(Entity, &DimensionTypeConfig), With<Dimension>>,
    world: WorldView,
    tables: Res<PhysicsTables>,
    baked: Res<BakedEnvDamage>,
    block_table: Res<EnvBlockTable>,
    world_border: Option<Res<DimensionWorldBorder>>,
    rules: Option<Res<GameRules>>,
    clock: Option<Res<BirthClock>>,
    mut pending: ResMut<PendingHits>,
    mut players: Populated<
        (
            Entity,
            &EntityUuid,
            &Transform,
            PlayerBody,
            &PlayerGameMode,
            &ClientOnGround,
            &ShiftKey,
            &Vitals,
            &mut PlayerFileState,
            &mut EnvTickState,
        ),
        With<Player>,
    >,
) {
    let Some((dimension_entity, dimension_config)) = dimensions.iter().next() else {
        return;
    };
    let dimension_view = world.get(dimension_entity);
    let border = world_border.as_deref().copied().unwrap_or_default();
    let bounds = border.bounds();
    let context = HazardContext {
        dimension_view: dimension_view.as_ref(),
        collision: tables.collision(),
        baked: &baked,
        block_table: &block_table,
        min_y: dimension_config.min_y,
        now: clock.as_deref().map_or(0, |birth| birth.0),
        fire_damage_enabled: rules
            .as_deref()
            .is_none_or(|game_rules| game_rules.is_on(GameRule::FireDamage)),
        border_bounds: WorldBorderBounds {
            west: bounds.min_x,
            north: bounds.min_z,
            east: bounds.max_x,
            south: bounds.max_z,
        },
        border_config: WorldBorderDamageConfig {
            safe_zone: border.damage_safe_zone,
            damage_per_block: border.damage_per_block,
        },
    };
    let mut sequence = u32::try_from(pending.len()).unwrap_or(u32::MAX);

    for (
        player,
        &uuid,
        transform,
        body,
        mode,
        on_ground,
        shift,
        vitals,
        file_state,
        mut tick_state,
    ) in &mut players
    {
        if vitals.is_alive() {
            let inputs = PlayerHazardInputs {
                player,
                uuid,
                position: transform.translation,
                body: body.dimensions(),
                mode: mode.0,
                on_ground: on_ground.0,
                shift: shift.0,
            };
            evaluate_player_hazards(
                &context,
                inputs,
                file_state,
                &tick_state,
                &mut sequence,
                &mut pending,
            );
        }
        if tick_state.tick_move_x != 0.0 || tick_state.tick_move_z != 0.0 {
            tick_state.set_if_neq(EnvTickState::default());
        }
    }
}
