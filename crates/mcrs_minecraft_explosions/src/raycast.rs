use alloc::vec::{self, Vec};
use glam::{DVec3, IVec3};
use mcrs_voxel::BlockPos;

use crate::interaction::{BlockInteraction, ExplosionRandom, ExplosionWorld};

const RAY_GRID_SIZE: i32 = 16;
const RAY_COUNT: usize = 1352;
const DEFAULT_TABLE_CAPACITY: usize = 4096;

#[derive(Clone, Copy)]
struct DeduplicationSlot {
    position: BlockPos,
    generation: u32,
}

const EMPTY_SLOT: DeduplicationSlot = DeduplicationSlot {
    position: BlockPos::new(0, 0, 0),
    generation: 0,
};

pub struct ExplosionScratch {
    ray_steps: Vec<DVec3>,
    slots: Vec<DeduplicationSlot>,
    generation: u32,
    affected_blocks: Vec<BlockPos>,
}

const _: () = {
    use core::mem::{align_of, size_of};
    assert!(
        size_of::<ExplosionScratch>() == 10 * size_of::<usize>()
            && align_of::<ExplosionScratch>() == align_of::<usize>(),
        "ExplosionScratch layout"
    );
};

impl Default for ExplosionScratch {
    fn default() -> Self {
        Self::new()
    }
}

impl ExplosionScratch {
    #[must_use]
    pub fn new() -> Self {
        Self {
            ray_steps: build_ray_steps(),
            slots: vec::from_elem(EMPTY_SLOT, DEFAULT_TABLE_CAPACITY),
            generation: 0,
            affected_blocks: Vec::with_capacity(2048),
        }
    }

    #[must_use]
    pub fn ray_count(&self) -> usize {
        self.ray_steps.len()
    }

    #[must_use]
    pub fn affected_blocks(&self) -> &[BlockPos] {
        &self.affected_blocks
    }

    fn insert_unique(&mut self, position: BlockPos) {
        let count = self.affected_blocks.len();
        if count.saturating_add(1).saturating_mul(2) > self.slots.len() {
            let capacity = self.slots.len().saturating_mul(2);
            let mut next = vec::from_elem(EMPTY_SLOT, capacity.max(DEFAULT_TABLE_CAPACITY));
            for &existing in &self.affected_blocks {
                probe_insert(&mut next, self.generation, existing);
            }
            self.slots = next;
        }
        if probe_insert(&mut self.slots, self.generation, position) {
            self.affected_blocks.push(position);
        }
    }
}

fn probe_insert(slots: &mut [DeduplicationSlot], generation: u32, position: BlockPos) -> bool {
    let mask = slots.len().saturating_sub(1);
    let hash = hash_block_position(position);
    for offset in 0..slots.len() {
        let Some(slot) = slots.get_mut(hash.wrapping_add(offset) & mask) else {
            return false;
        };
        if slot.generation != generation {
            *slot = DeduplicationSlot {
                position,
                generation,
            };
            return true;
        }
        if slot.position == position {
            return false;
        }
    }
    false
}

fn build_ray_steps() -> Vec<DVec3> {
    let mut steps = Vec::with_capacity(RAY_COUNT);
    let max_coordinate = f64::from(RAY_GRID_SIZE - 1);
    for grid_x in 0..RAY_GRID_SIZE {
        for grid_y in 0..RAY_GRID_SIZE {
            for grid_z in 0..RAY_GRID_SIZE {
                let on_surface = [grid_x, grid_y, grid_z]
                    .into_iter()
                    .any(|c| c == 0 || c == 15);
                if on_surface {
                    let raw = IVec3::new(grid_x, grid_y, grid_z).as_dvec3() / max_coordinate * 2.0
                        - DVec3::ONE;
                    steps.push((raw / libm::sqrt(raw.length_squared())) * 0.3);
                }
            }
        }
    }
    debug_assert!(steps.len() == RAY_COUNT, "ray grid must contain 1352 rays");
    steps
}

#[inline]
fn hash_block_position(position: BlockPos) -> usize {
    let mut hash = (i64::from(position.x) as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    hash ^= (i64::from(position.y) as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    hash = hash.rotate_left(27).wrapping_mul(0x94D0_49BB_1331_11EB);
    hash ^= (i64::from(position.z) as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93);
    hash ^= hash >> 30;
    (hash.wrapping_mul(0xBF58_476D_1CE4_E5B9) ^ (hash >> 27)) as usize
}

#[inline]
fn floor_block_position(cursor: DVec3) -> Option<BlockPos> {
    if !cursor.is_finite() {
        return None;
    }
    let floored = cursor.floor();
    let range = f64::from(i32::MIN)..=f64::from(i32::MAX);
    (range.contains(&floored.x) && range.contains(&floored.y) && range.contains(&floored.z))
        .then_some(BlockPos::new(
            floored.x as i32,
            floored.y as i32,
            floored.z as i32,
        ))
}

pub fn collect_affected_blocks<'scratch>(
    center: DVec3,
    radius: f32,
    interaction: BlockInteraction,
    world: &impl ExplosionWorld,
    random: &mut impl ExplosionRandom,
    scratch: &'scratch mut ExplosionScratch,
) -> &'scratch [BlockPos] {
    scratch.affected_blocks.clear();
    if scratch.generation == u32::MAX {
        scratch.slots.fill(EMPTY_SLOT);
        scratch.generation = 1;
    } else {
        scratch.generation += 1;
    }
    if !interaction.collects_blocks() || !radius.is_finite() || radius <= 0.0 || !center.is_finite()
    {
        return &scratch.affected_blocks;
    }
    for ray_index in 0..scratch.ray_steps.len() {
        let Some(step) = scratch.ray_steps.get(ray_index).copied() else {
            break;
        };
        let mut strength = radius * (0.7 + random.next_f32() * 0.6);
        let mut cursor = center;
        while strength > 0.0 {
            let Some(block_position) = floor_block_position(cursor) else {
                break;
            };
            if !world.is_in_bounds(block_position) {
                break;
            }
            if let Some(resistance) = world.block_and_fluid_resistance(block_position) {
                if resistance.is_infinite() && resistance.is_sign_positive() {
                    break;
                }
                let effective = if resistance.is_finite() {
                    resistance.max(0.0)
                } else {
                    0.0
                };
                strength -= (effective + 0.3) * 0.3;
            }
            if strength > 0.0 {
                scratch.insert_unique(block_position);
            }
            cursor += step;
            strength -= 0.225;
        }
    }
    &scratch.affected_blocks
}
