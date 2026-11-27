use glam::DVec3;
use mcrs_voxel::BlockPos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockInteraction {
    None,
    Destroy,
    DestroyWithDecay,
    TriggerBlock,
}

impl BlockInteraction {
    #[must_use]
    pub const fn collects_blocks(self) -> bool {
        !matches!(self, Self::None)
    }

    #[must_use]
    pub const fn destroys_blocks(self) -> bool {
        matches!(self, Self::Destroy | Self::DestroyWithDecay)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

pub trait ExplosionRandom {
    fn next_f32(&mut self) -> f32;
    fn next_f64(&mut self) -> f64;
    fn next_u32_bounded(&mut self, bound: u32) -> u32;
}

pub trait ExplosionWorld {
    fn is_in_bounds(&self, position: BlockPos) -> bool;
    fn block_and_fluid_resistance(&self, position: BlockPos) -> Option<f32>;
    fn is_segment_occluded(&self, from: DVec3, to: DVec3) -> bool;
}

const _: () = {
    use core::mem::{align_of, size_of};
    assert!(
        size_of::<BlockInteraction>() == 1 && align_of::<BlockInteraction>() == 1,
        "BlockInteraction layout"
    );
    assert!(
        size_of::<Difficulty>() == 1 && align_of::<Difficulty>() == 1,
        "Difficulty layout"
    );
};
