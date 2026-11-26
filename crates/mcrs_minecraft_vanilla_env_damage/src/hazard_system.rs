use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::prelude::{DetectChangesMut, Has, Mut, Populated, Query, Res, ResMut, With};
use bevy_ecs::system::SystemParam;
use glam::DVec3;
use mcrs_minecraft_damage::{DamageSource, Hit, Vitals};
use mcrs_minecraft_engine::client_on_ground::ClientOnGround;
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine::{self as engine, OwnEquipment, PlayerBody, PlayerLivedTicks, ShiftKey};
use mcrs_minecraft_engine_damage::{DamageRequest, PendingHits};
use mcrs_minecraft_engine_physics::PhysicsTables;
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::block_update::BlockSetRequest;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::game_rules::GameRules;
use mcrs_minecraft_engine_world::world::dimension::{
    Dimension, DimensionTypeConfig, HasSkyLight, HasWeather,
};
use mcrs_minecraft_engine_world::world::storage::section::SectionIndex;
use mcrs_minecraft_engine_world::world::storage::view::{DimensionView, WorldView};
use mcrs_minecraft_engine_world::world_border::DimensionWorldBorder;
use mcrs_minecraft_entity_attribute::AttributeInstances;
use mcrs_minecraft_entity_key::{EntityUuid, WriterKey};
use mcrs_minecraft_env_damage::{
    self as env_damage, AxisAlignedBounds, CACTUS_DAMAGE, CAMPFIRE_DAMAGE, CONTACT_BOX_SHRINK,
    ContactHazard, FreezeImmunity, IN_WALL_DAMAGE, OnFireState, PowderSnowExposure,
    SOUL_CAMPFIRE_DAMAGE, SuffocationState, WorldBorderBounds, WorldBorderDamageConfig,
};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_rule::GameRule;
use mcrs_minecraft_physics::{
    self as physics, Aabb, BlockFlags, Collider, CollisionTable, ShapeClass,
};
use mcrs_minecraft_pose::Dimensions;
use mcrs_voxel::{BlockPos, SectionPos};

use crate::bake::{BakedEnvDamage, EnvBlockTable, EnvContactBlock};
use crate::env_state::{EnvTickState, PendingEnvEffect};
use crate::fall_system;
use crate::registry_tables::EnvRegistryTables;
use crate::status_hazards::{
    self, ExtinguishStatus, PlayerFireOutcome, PowderSnowFallReset, RainContext, RainWorld,
    StatusInputs, SurroundingsScan,
};
use crate::water_hazards::{self, DrownInputs};

#[derive(SystemParam)]
pub(crate) struct EnvWorldAccess<'w, 's> {
    dimensions: Query<
        'w,
        's,
        (
            Entity,
            &'static DimensionTypeConfig,
            Has<HasWeather>,
            Has<HasSkyLight>,
            Option<&'static SectionIndex>,
        ),
        With<Dimension>,
    >,
    world: WorldView<'w, 's>,
    rain: RainWorld<'w, 's>,
    world_border: Option<Res<'w, DimensionWorldBorder>>,
    rules: Option<Res<'w, GameRules>>,
    clock: Option<Res<'w, BirthClock>>,
}

#[derive(SystemParam)]
pub(crate) struct EnvHazardOutputs<'w> {
    pending: ResMut<'w, PendingHits>,
    signals: MessageWriter<'w, PlayerSignal>,
    block_requests: MessageWriter<'w, BlockSetRequest>,
    effects: MessageWriter<'w, PendingEnvEffect>,
}

struct DimensionSnapshot<'a> {
    entity: Entity,
    config: &'a DimensionTypeConfig,
    section_index: Option<&'a SectionIndex>,
    view: Option<&'a DimensionView<'a>>,
    has_weather: bool,
    has_sky_light: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SleepState {
    Awake,
    Sleeping,
}

struct HazardContext<'a> {
    dimension_entity: Entity,
    dimension_view: Option<&'a DimensionView<'a>>,
    collision: &'a CollisionTable,
    baked: &'a BakedEnvDamage,
    block_table: &'a EnvBlockTable,
    rain: RainContext<'a>,
    min_y: i32,
    now: u64,
    drowning_damage_enabled: bool,
    fire_damage_enabled: bool,
    freeze_damage_enabled: bool,
    border_bounds: WorldBorderBounds,
    border_config: WorldBorderDamageConfig,
}

struct PlayerHazardInputs<'a> {
    player: Entity,
    uuid: EntityUuid,
    position: DVec3,
    body: Dimensions,
    mode: GameMode,
    on_ground: bool,
    shift: bool,
    lived_ticks: Option<u64>,
    equipment: Option<&'a OwnEquipment>,
    attributes: Option<&'a AttributeInstances>,
}

struct PlayerEvaluationFrame<'a> {
    inputs: &'a PlayerHazardInputs<'a>,
    status_inputs: StatusInputs<'a>,
    surroundings: SurroundingsScan,
    shrunk: Aabb,
    tick_state: &'a EnvTickState,
    fire_outcome: PlayerFireOutcome,
}

const _: () = {
    assert!(
        size_of::<DimensionSnapshot<'static>>() == 40,
        "DimensionSnapshot size must match dimension references and flags"
    );
    assert!(
        align_of::<DimensionSnapshot<'static>>() == 8,
        "DimensionSnapshot alignment must match pointer alignment"
    );
    assert!(
        size_of::<SleepState>() == 1,
        "SleepState size must match single-byte enum"
    );
    assert!(
        align_of::<SleepState>() == 1,
        "SleepState alignment must match single-byte enum"
    );
    assert!(
        size_of::<HazardContext<'static>>() == 152,
        "HazardContext size must match context references and rule flags"
    );
    assert!(
        align_of::<HazardContext<'static>>() == 8,
        "HazardContext alignment must match pointer alignment"
    );
    assert!(
        size_of::<PlayerHazardInputs<'static>>() == 104,
        "PlayerHazardInputs size must match player frame data"
    );
    assert!(
        align_of::<PlayerHazardInputs<'static>>() == 8,
        "PlayerHazardInputs alignment must match f64 alignment"
    );
    assert!(
        size_of::<PlayerEvaluationFrame<'static>>() == 512,
        "PlayerEvaluationFrame size must match bundled per-player frame state"
    );
    assert!(
        align_of::<PlayerEvaluationFrame<'static>>() == 8,
        "PlayerEvaluationFrame alignment must match pointer and f64 alignment"
    );
};

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

fn collect_contact_hazards(
    context: &HazardContext<'_>,
    inputs: &PlayerHazardInputs<'_>,
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
    inputs: &PlayerHazardInputs<'_>,
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

fn collect_suffocation_and_border(
    context: &HazardContext<'_>,
    inputs: &PlayerHazardInputs<'_>,
    eye: DVec3,
    sleep_state: SleepState,
    sequence: &mut u32,
    pending: &mut PendingHits,
) {
    let suffocation = if sleep_state == SleepState::Awake
        && is_suffocating(
            context.dimension_view,
            context.collision,
            eye,
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
}

fn apply_drown(
    context: &HazardContext<'_>,
    inputs: &PlayerHazardInputs<'_>,
    eye: DVec3,
    mut file_state: Mut<'_, PlayerFileState>,
    sequence: &mut u32,
    outputs: &mut EnvHazardOutputs<'_>,
) {
    let drown_inputs = DrownInputs {
        player: inputs.player,
        eye,
        mode: inputs.mode,
        now: context.now,
        drowning_damage_enabled: context.drowning_damage_enabled,
        attributes: inputs.attributes,
    };
    if let Some(evaluation) = water_hazards::step_player_drowning(
        context.dimension_view,
        context.block_table,
        &drown_inputs,
        &file_state,
        &mut outputs.signals,
    ) {
        if file_state.air != evaluation.next_air {
            file_state
                .reborrow()
                .map_unchanged(|state| &mut state.air)
                .set_if_neq(evaluation.next_air);
        }
        if let Some(damage) = evaluation.damage {
            push_env_hit(
                &mut outputs.pending,
                inputs.player,
                inputs.uuid,
                context.now,
                sequence,
                context.baked.drown,
                damage,
            );
        }
    }
}

fn evaluate_spectator_hazards(
    context: &HazardContext<'_>,
    inputs: &PlayerHazardInputs<'_>,
    freeze_age: u64,
    mut file_state: Mut<'_, PlayerFileState>,
    tick_state: &EnvTickState,
    sequence: &mut u32,
    outputs: &mut EnvHazardOutputs<'_>,
) -> u8 {
    let next_fire_ticks = if file_state.fire > 0 {
        file_state.fire - 1
    } else {
        file_state.fire
    };
    if file_state.fire != next_fire_ticks {
        file_state
            .reborrow()
            .map_unchanged(|state| &mut state.fire)
            .set_if_neq(next_fire_ticks);
    }

    if let Some(damage) = env_damage::void_damage(inputs.position.y, context.min_y) {
        push_env_hit(
            &mut outputs.pending,
            inputs.player,
            inputs.uuid,
            context.now,
            sequence,
            context.baked.out_of_world,
            damage,
        );
    }

    let eye = engine::eye_position(inputs.position, inputs.body);
    apply_drown(
        context,
        inputs,
        eye,
        file_state.reborrow(),
        sequence,
        outputs,
    );

    let on_fire_state = if next_fire_ticks > 0 {
        OnFireState::Burning
    } else {
        OnFireState::NotBurning
    };
    let freeze_outcome = env_damage::step_freeze(
        file_state.ticks_frozen,
        PowderSnowExposure::Outside,
        on_fire_state,
        freeze_age,
        FreezeImmunity::ColdImmune,
    );
    if file_state.ticks_frozen != freeze_outcome.next_ticks_frozen {
        file_state
            .reborrow()
            .map_unchanged(|state| &mut state.ticks_frozen)
            .set_if_neq(freeze_outcome.next_ticks_frozen);
    }

    tick_state.extinguish_sound_cooldown.saturating_sub(1)
}

fn push_fire_and_lava_hits(
    context: &HazardContext<'_>,
    inputs: &PlayerHazardInputs<'_>,
    fire_outcome: PlayerFireOutcome,
    sequence: &mut u32,
    pending: &mut PendingHits,
) {
    if !context.fire_damage_enabled {
        return;
    }
    if let Some(damage) = fire_outcome.in_fire_damage {
        push_env_hit(
            pending,
            inputs.player,
            inputs.uuid,
            context.now,
            sequence,
            context.baked.in_fire,
            damage,
        );
    }
    if let Some(damage) = fire_outcome.lava_damage {
        push_env_hit(
            pending,
            inputs.player,
            inputs.uuid,
            context.now,
            sequence,
            context.baked.lava,
            damage,
        );
    }
}

fn apply_freeze_hazard(
    context: &HazardContext<'_>,
    frame: &PlayerEvaluationFrame<'_>,
    mut file_state: Mut<'_, PlayerFileState>,
    sequence: &mut u32,
    pending: &mut PendingHits,
) -> ExtinguishStatus {
    let evaluation = status_hazards::step_player_freezing(
        &frame.status_inputs,
        &file_state,
        &frame.surroundings,
        frame.fire_outcome,
        context.rain.registry_tables,
    );
    if file_state.ticks_frozen != evaluation.outcome.next_ticks_frozen {
        file_state
            .reborrow()
            .map_unchanged(|state| &mut state.ticks_frozen)
            .set_if_neq(evaluation.outcome.next_ticks_frozen);
    }
    if let Some(damage) = evaluation.outcome.damage {
        push_env_hit(
            pending,
            frame.inputs.player,
            frame.inputs.uuid,
            context.now,
            sequence,
            context.baked.freeze,
            damage,
        );
    }
    evaluation.extinguish_status
}

fn collect_step_eight_and_freeze(
    context: &HazardContext<'_>,
    frame: &PlayerEvaluationFrame<'_>,
    mut file_state: Mut<'_, PlayerFileState>,
    sequence: &mut u32,
    outputs: &mut EnvHazardOutputs<'_>,
) -> u8 {
    check_hot_floor(context, frame.inputs, sequence, &mut outputs.pending);
    push_fire_and_lava_hits(
        context,
        frame.inputs,
        frame.fire_outcome,
        sequence,
        &mut outputs.pending,
    );
    collect_contact_hazards(
        context,
        frame.inputs,
        frame.shrunk,
        frame.tick_state,
        sequence,
        &mut outputs.pending,
    );

    if frame.fire_outcome.extinguish_status == ExtinguishStatus::Extinguished {
        if !frame.surroundings.powder_snow.as_slice().is_empty() {
            status_hazards::emit_powder_snow_melt(
                context.dimension_entity,
                &frame.surroundings.powder_snow,
                &mut outputs.block_requests,
            );
        }
        if !frame.surroundings.cauldrons.as_slice().is_empty() {
            status_hazards::emit_cauldron_extinguish(
                context.dimension_entity,
                context.block_table,
                &frame.surroundings.cauldrons,
                &mut outputs.block_requests,
            );
        }
    }

    let freeze_extinguish = apply_freeze_hazard(
        context,
        frame,
        file_state.reborrow(),
        sequence,
        &mut outputs.pending,
    );
    let combined_extinguish = if frame.fire_outcome.extinguish_status
        == ExtinguishStatus::Extinguished
        || freeze_extinguish == ExtinguishStatus::Extinguished
    {
        ExtinguishStatus::Extinguished
    } else {
        ExtinguishStatus::Unchanged
    };
    status_hazards::step_extinguish_sound(
        &frame.status_inputs,
        context.baked,
        &file_state,
        frame.tick_state.extinguish_sound_cooldown,
        combined_extinguish,
        &mut outputs.effects,
    )
}

fn apply_base_tick_fire_and_void(
    context: &HazardContext<'_>,
    inputs: &PlayerHazardInputs<'_>,
    surroundings: &SurroundingsScan,
    mut file_state: Mut<'_, PlayerFileState>,
    sequence: &mut u32,
    pending: &mut PendingHits,
) -> PlayerFireOutcome {
    let fire_outcome = status_hazards::step_player_fire(&file_state, surroundings);
    if file_state.fire != fire_outcome.next_fire_ticks {
        file_state
            .reborrow()
            .map_unchanged(|state| &mut state.fire)
            .set_if_neq(fire_outcome.next_fire_ticks);
    }
    if context.fire_damage_enabled
        && let Some(damage) = fire_outcome.burning_damage
    {
        push_env_hit(
            pending,
            inputs.player,
            inputs.uuid,
            context.now,
            sequence,
            context.baked.on_fire,
            damage,
        );
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
    fire_outcome
}

fn evaluate_player_hazards(
    context: &HazardContext<'_>,
    rain_world: &RainWorld<'_, '_>,
    inputs: &PlayerHazardInputs<'_>,
    mut file_state: Mut<'_, PlayerFileState>,
    tick_state: &EnvTickState,
    sequence: &mut u32,
    outputs: &mut EnvHazardOutputs<'_>,
) -> u8 {
    let freeze_age = inputs.lived_ticks.unwrap_or(tick_state.freeze_age_ticks);
    if inputs.mode == GameMode::Spectator {
        return evaluate_spectator_hazards(
            context, inputs, freeze_age, file_state, tick_state, sequence, outputs,
        );
    }

    let sleep_state = if file_state.sleeping_pos.is_some() {
        SleepState::Sleeping
    } else {
        SleepState::Awake
    };
    let eye = engine::eye_position(inputs.position, inputs.body);
    let shrunk = Aabb::of_body(inputs.position, inputs.body.width, inputs.body.height)
        .deflate(CONTACT_BOX_SHRINK);
    let status_inputs = StatusInputs {
        player: inputs.player,
        position: inputs.position,
        mode: inputs.mode,
        now: context.now,
        freeze_age,
        freeze_damage_enabled: context.freeze_damage_enabled,
        equipment: inputs.equipment,
    };
    let surroundings = status_hazards::scan_surroundings(
        &context.rain,
        rain_world,
        inputs.position,
        inputs.body,
        shrunk,
    );

    if file_state.fall_distance > 0.0
        && (surroundings.reset_fall_in_powder_snow == PowderSnowFallReset::Reset
            || fall_system::box_has_block_flag(
                context.dimension_view,
                context.collision,
                shrunk,
                BlockFlags::WATER | BlockFlags::FALL_DAMAGE_RESETTING,
            ))
    {
        file_state
            .reborrow()
            .map_unchanged(|state| &mut state.fall_distance)
            .set_if_neq(0.0);
    }

    let fire_outcome = apply_base_tick_fire_and_void(
        context,
        inputs,
        &surroundings,
        file_state.reborrow(),
        sequence,
        &mut outputs.pending,
    );
    collect_suffocation_and_border(
        context,
        inputs,
        eye,
        sleep_state,
        sequence,
        &mut outputs.pending,
    );
    apply_drown(
        context,
        inputs,
        eye,
        file_state.reborrow(),
        sequence,
        outputs,
    );

    let frame = PlayerEvaluationFrame {
        inputs,
        status_inputs,
        surroundings,
        shrunk,
        tick_state,
        fire_outcome,
    };
    collect_step_eight_and_freeze(context, &frame, file_state, sequence, outputs)
}

fn build_hazard_context<'a>(
    dimension: DimensionSnapshot<'a>,
    access: &'a EnvWorldAccess<'_, '_>,
    tables: &'a PhysicsTables,
    baked: &'a BakedEnvDamage,
    block_table: &'a EnvBlockTable,
    registry_tables: &'a EnvRegistryTables,
) -> HazardContext<'a> {
    let border = access.world_border.as_deref().copied().unwrap_or_default();
    let bounds = border.bounds();
    let max_y = (dimension.config.min_y
        + dimension.config.section_count.cast_signed() * (SectionPos::SIZE as i32)
        - 1)
    .max(dimension.config.min_y);
    let has_rain = dimension.has_weather
        && dimension.has_sky_light
        && access
            .rain
            .weather_level
            .as_deref()
            .is_some_and(|level| level.rain > 0.2_f32);
    let rain = RainContext {
        has_rain,
        min_y: dimension.config.min_y,
        max_y,
        dimension_view: dimension.view,
        block_table,
        registry_tables,
        section_index: dimension.section_index,
    };
    HazardContext {
        dimension_entity: dimension.entity,
        dimension_view: dimension.view,
        collision: tables.collision(),
        baked,
        block_table,
        rain,
        min_y: dimension.config.min_y,
        now: access.clock.as_deref().map_or(0, |birth| birth.0),
        drowning_damage_enabled: access
            .rules
            .as_deref()
            .is_none_or(|game_rules| game_rules.is_on(GameRule::DrowningDamage)),
        fire_damage_enabled: access
            .rules
            .as_deref()
            .is_none_or(|game_rules| game_rules.is_on(GameRule::FireDamage)),
        freeze_damage_enabled: access
            .rules
            .as_deref()
            .is_none_or(|game_rules| game_rules.is_on(GameRule::FreezeDamage)),
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
    }
}

#[expect(
    clippy::type_complexity,
    reason = "Player query bundles components required to evaluate environmental hazards"
)]
pub(crate) fn collect_env_hazards(
    access: EnvWorldAccess<'_, '_>,
    tables: Res<PhysicsTables>,
    baked: Res<BakedEnvDamage>,
    block_table: Res<EnvBlockTable>,
    registry_tables: Res<EnvRegistryTables>,
    mut outputs: EnvHazardOutputs<'_>,
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
            Option<&PlayerLivedTicks>,
            Option<&OwnEquipment>,
            Option<&AttributeInstances>,
            &mut PlayerFileState,
            &mut EnvTickState,
        ),
        With<Player>,
    >,
) {
    let Some((dimension_entity, dimension_config, has_weather, has_sky_light, section_index)) =
        access.dimensions.iter().next()
    else {
        return;
    };
    let dimension_view = access.world.get(dimension_entity);
    let snapshot = DimensionSnapshot {
        entity: dimension_entity,
        config: dimension_config,
        section_index,
        view: dimension_view.as_ref(),
        has_weather,
        has_sky_light,
    };
    let context = build_hazard_context(
        snapshot,
        &access,
        &tables,
        &baked,
        &block_table,
        &registry_tables,
    );
    let mut sequence = u32::try_from(outputs.pending.len()).unwrap_or(u32::MAX);

    for (
        player,
        &uuid,
        transform,
        body,
        mode,
        on_ground,
        shift,
        vitals,
        lived_ticks,
        equipment,
        attributes,
        file_state,
        mut tick_state,
    ) in &mut players
    {
        let next_cooldown = if vitals.is_alive() {
            let inputs = PlayerHazardInputs {
                player,
                uuid,
                position: transform.translation,
                body: body.dimensions(),
                mode: mode.0,
                on_ground: on_ground.0,
                shift: shift.0,
                lived_ticks: lived_ticks.map(|ticks| ticks.0),
                equipment,
                attributes,
            };
            evaluate_player_hazards(
                &context,
                &access.rain,
                &inputs,
                file_state,
                &tick_state,
                &mut sequence,
                &mut outputs,
            )
        } else {
            tick_state.extinguish_sound_cooldown.saturating_sub(1)
        };
        let next_freeze_age_ticks = if lived_ticks.is_none() {
            tick_state.freeze_age_ticks.saturating_add(1)
        } else {
            tick_state.freeze_age_ticks
        };
        let next_tick_state = EnvTickState {
            tick_move_x: 0.0,
            tick_move_z: 0.0,
            freeze_age_ticks: next_freeze_age_ticks,
            extinguish_sound_cooldown: next_cooldown,
        };
        tick_state.set_if_neq(next_tick_state);
    }
}
