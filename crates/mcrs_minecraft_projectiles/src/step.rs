use glam::DVec3;
use mcrs_minecraft_entity_key::EntityId;
use mcrs_voxel::{BlockPos, BlockStateId};

use crate::{
    EntityTarget, FlightCategory, ProjectileAabb, ProjectileMedium, ProjectileRandom,
    ProjectileState, ProjectileWorld, has_layout, step_rotation,
};

pub const ARROW_DESPAWN_TICKS: u16 = 1200;
const MAX_RAYCAST_CELLS: usize = 4096;
const BLOCK_COORD_LIMIT: f64 = 2_000_000_000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct BlockHit {
    pub block_position: BlockPos,
    pub block_state: BlockStateId,
    pub point: DVec3,
    pub fraction: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct EntityHit {
    pub entity_id: EntityId,
    pub point: DVec3,
    pub distance_squared: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum StepOutcome {
    InGround,
    DespawnedInGround,
    ReleasedFromGround,
    Flew,
    HitBlock(BlockHit),
    StuckInBlock(BlockHit),
    HitEntity {
        hit: EntityHit,
        next_position: DVec3,
    },
}

#[must_use]
pub fn hit_margin(age_ticks: u32) -> f64 {
    f64::from(((age_ticks as f32 - 2.0) / 20.0).clamp(0.0, 0.3))
}

struct AxisStepper {
    cell: i32,
    step: i32,
    t_max: f64,
    t_delta: f64,
}

fn dir_sign(value: f64) -> i32 {
    i32::from(value > 0.0) - i32::from(value < 0.0)
}

fn init_axis(origin: f64, delta: f64) -> AxisStepper {
    let (cell, step) = (libm::floor(origin) as i32, dir_sign(delta));
    let edge = f64::from(cell + i32::from(step > 0));
    let (t_max, t_delta) = if step == 0 {
        (f64::INFINITY, f64::INFINITY)
    } else {
        ((edge - origin) / delta, f64::from(step) / delta)
    };
    AxisStepper {
        cell,
        step,
        t_max,
        t_delta,
    }
}

fn step_axis(axis: &mut AxisStepper) -> bool {
    axis.cell.checked_add(axis.step).is_some_and(|next| {
        (axis.cell, axis.t_max) = (next, axis.t_max + axis.t_delta);
        true
    })
}

fn probe_inside_block(world: &(impl ProjectileWorld + ?Sized), position: DVec3) -> bool {
    if !position.is_finite() || position.abs().max_element() > BLOCK_COORD_LIMIT {
        return false;
    }
    let probe = ProjectileAabb::new(position, position).inflate(0.06);
    let (lower, upper) = (probe.min.floor().as_ivec3(), probe.max.floor().as_ivec3());
    let mut hit = false;
    for cell_x in lower.x..=upper.x {
        for cell_y in lower.y..=upper.y {
            for cell_z in lower.z..=upper.z {
                let cell = BlockPos::new(cell_x, cell_y, cell_z);
                let offset = cell.as_dvec3();
                world.visit_block_boxes(cell, |local| {
                    hit |= ProjectileAabb::new(local.min + offset, local.max + offset)
                        .intersects(&probe);
                    !hit
                });
            }
        }
    }
    hit
}

#[must_use]
pub fn raycast_blocks(
    world: &(impl ProjectileWorld + ?Sized),
    start: DVec3,
    end: DVec3,
) -> Option<BlockHit> {
    let delta = end - start;
    if !start.is_finite() || !delta.is_finite() || start.abs().max_element() > BLOCK_COORD_LIMIT {
        return None;
    }
    let (mut axis_x, mut axis_y, mut axis_z) = (
        init_axis(start.x, delta.x),
        init_axis(start.y, delta.y),
        init_axis(start.z, delta.z),
    );
    let mut best = None::<BlockHit>;
    for _ in 0..MAX_RAYCAST_CELLS {
        let block_position = BlockPos::new(axis_x.cell, axis_y.cell, axis_z.cell);
        let offset = block_position.as_dvec3();
        world.visit_block_boxes(block_position, |local| {
            let world_box = ProjectileAabb::new(local.min + offset, local.max + offset);
            if let Some(fraction) = world_box.clip_segment(start, end)
                && fraction.is_finite()
                && best.is_none_or(|current| fraction < current.fraction)
            {
                best = Some(BlockHit {
                    block_position,
                    block_state: world.block_state(block_position),
                    point: start + delta * fraction,
                    fraction,
                });
            }
            true
        });
        let step_t = axis_x.t_max.min(axis_y.t_max).min(axis_z.t_max);
        if !step_t.is_finite() || step_t > 1.0 || best.is_some_and(|hit| step_t > hit.fraction) {
            break;
        }
        let axis = if axis_x.t_max <= axis_y.t_max && axis_x.t_max <= axis_z.t_max {
            &mut axis_x
        } else if axis_y.t_max <= axis_z.t_max {
            &mut axis_y
        } else {
            &mut axis_z
        };
        if !step_axis(axis) {
            break;
        }
    }
    best
}

pub fn finish_arrow_flight_step(state: &mut ProjectileState, next_position: DVec3) {
    if state.removed || state.in_ground || !next_position.is_finite() {
        return;
    }
    state.position = next_position;
    let spec = state.kind.spec();
    if !state.in_water {
        state.velocity *= f64::from(spec.air_drag);
    }
    if !state.no_gravity {
        state.velocity.y -= spec.gravity;
    }
}

fn update_rotation_and_owner(state: &mut ProjectileState, targets: &[EntityTarget]) {
    let angles = step_rotation(state.yaw, state.pitch, state.velocity);
    (state.yaw, state.pitch) = (angles.yaw, angles.pitch);
    if state.left_owner || state.owner.is_none() {
        state.left_owner = true;
        return;
    }
    let bounds = state.bounds();
    let sweep = ProjectileAabb::new(
        bounds.min + state.velocity.min(DVec3::ZERO),
        bounds.max + state.velocity.max(DVec3::ZERO),
    )
    .inflate(1.0);
    state.left_owner = !targets
        .iter()
        .any(|entry| entry.aimable && entry.in_owner_group && sweep.intersects(&entry.bounds));
}

fn stick_in_block(state: &mut ProjectileState, hit: BlockHit) -> StepOutcome {
    (state.velocity, state.in_ground, state.despawn_ticks) = (DVec3::ZERO, true, 0);
    state.in_block_position = Some(hit.block_position);
    state.in_block_state = Some(hit.block_state);
    StepOutcome::StuckInBlock(hit)
}

fn step_in_ground(
    state: &mut ProjectileState,
    world: &(impl ProjectileWorld + ?Sized),
    random: &mut (impl ProjectileRandom + ?Sized),
) -> StepOutcome {
    let state_changed = state
        .in_block_position
        .zip(state.in_block_state)
        .is_none_or(|(block, want)| world.block_state(block) != want);
    if state_changed && !probe_inside_block(world, state.position) {
        let mut scale = || f64::from(random.next_f32() * 0.2);
        state.velocity *= DVec3::new(scale(), scale(), scale());
        (state.in_ground, state.despawn_ticks) = (false, 0);
        return StepOutcome::ReleasedFromGround;
    }
    state.despawn_ticks = state.despawn_ticks.saturating_add(1);
    if state.despawn_ticks >= ARROW_DESPAWN_TICKS {
        state.removed = true;
        return StepOutcome::DespawnedInGround;
    }
    StepOutcome::InGround
}

pub fn step_projectile(
    state: &mut ProjectileState,
    world: &(impl ProjectileWorld + ?Sized),
    targets: &[EntityTarget],
    random: &mut (impl ProjectileRandom + ?Sized),
) -> StepOutcome {
    state.age_ticks = state.age_ticks.saturating_add(1);
    state.shake_ticks = state.shake_ticks.saturating_sub(1);
    let spec = state.kind.spec();
    let is_arrow = spec.category == FlightCategory::Arrow;
    if is_arrow && state.returning {
        return StepOutcome::Flew;
    }
    if is_arrow && state.in_ground {
        return step_in_ground(state, world, random);
    }
    let (start, move_velocity) = (state.position, state.velocity);
    if !start.is_finite() || !move_velocity.is_finite() {
        return StepOutcome::Flew;
    }
    if is_arrow && let Some(hit) = raycast_blocks(world, start, start) {
        return stick_in_block(state, hit);
    }
    state.in_water = world.medium_at(start, state.bounds()) == ProjectileMedium::Water;
    let (air, water) = (f64::from(spec.air_drag), f64::from(spec.water_drag));
    let delta = if !is_arrow {
        if !state.no_gravity {
            state.velocity.y -= spec.gravity;
        }
        state.velocity *= if state.in_water { water } else { air };
        state.velocity
    } else {
        if state.in_water {
            state.velocity *= water;
        }
        update_rotation_and_owner(state, targets);
        move_velocity
    };
    let raw_end = start + delta;
    if !raw_end.is_finite() {
        return StepOutcome::Flew;
    }
    let block_hit = raycast_blocks(world, start, raw_end);
    let segment_end = block_hit.map_or(raw_end, |hit| hit.point);
    let entity_hit = sweep_entity_hit(state, world, targets, start, segment_end);
    if !is_arrow {
        state.position = entity_hit.map_or(segment_end, |hit| hit.point);
        update_rotation_and_owner(state, targets);
    }
    if let Some(hit) = entity_hit {
        let next_position = if is_arrow { segment_end } else { hit.point };
        return StepOutcome::HitEntity { hit, next_position };
    }
    if !is_arrow {
        return block_hit.map_or(StepOutcome::Flew, StepOutcome::HitBlock);
    }
    if let Some(hit) = block_hit {
        let axis = DVec3::new(
            f64::from(dir_sign(move_velocity.x)),
            f64::from(dir_sign(move_velocity.y)),
            f64::from(dir_sign(move_velocity.z)),
        );
        state.position = hit.point - axis * f64::from(0.05_f32);
        (state.shake_ticks, state.critical, state.pierce_level) = (7, false, 0);
        return stick_in_block(state, hit);
    }
    finish_arrow_flight_step(state, raw_end);
    StepOutcome::Flew
}

#[must_use]
pub fn sweep_entity_hit(
    state: &ProjectileState,
    world: &(impl ProjectileWorld + ?Sized),
    targets: &[EntityTarget],
    start: DVec3,
    end: DVec3,
) -> Option<EntityHit> {
    let delta = end - start;
    if !start.is_finite() || !end.is_finite() || !delta.is_finite() {
        return None;
    }
    let is_arrow = state.kind.spec().category == FlightCategory::Arrow;
    let margin = hit_margin(state.age_ticks);
    let mut best = None::<EntityHit>;
    for entry in targets {
        if !entry.aimable || !entry.bounds.is_valid() {
            continue;
        }
        let pierced = is_arrow && state.pierce_level > 0 && state.pierced.contains(&Some(entry.id));
        if (entry.in_owner_group && !state.left_owner)
            || (is_arrow && !entry.arrow_harm_allowed)
            || pierced
        {
            continue;
        }
        let bounds = entry.bounds;
        let point = if is_arrow && let Some(fraction) = bounds.clip_segment(start, end) {
            start + delta * fraction
        } else if (!is_arrow || margin > 0.0)
            && let Some(fraction) = bounds.inflate(margin).clip_segment(start, end)
        {
            let clip_point = start + delta * fraction;
            let center = (bounds.min + bounds.max) * 0.5;
            let clear = !is_arrow
                || raycast_blocks(world, clip_point, center).is_none_or(|wall| {
                    bounds
                        .clip_segment(clip_point, center)
                        .is_some_and(|bound| wall.fraction >= bound)
                        || wall.fraction >= 1.0
                });
            if !clear {
                continue;
            }
            clip_point
        } else {
            continue;
        };
        let reference = if is_arrow { entry.position } else { point };
        let distance_squared = start.distance_squared(reference);
        if distance_squared.is_finite()
            && best.is_none_or(|current| distance_squared < current.distance_squared)
        {
            best = Some(EntityHit {
                entity_id: entry.id,
                point,
                distance_squared,
            });
        }
    }
    best
}

const _: () = assert!(
    has_layout::<BlockHit>(48, 8)
        && has_layout::<EntityHit>(40, 8)
        && has_layout::<StepOutcome>(72, 8),
    "step hit types must keep expected size and alignment"
);
