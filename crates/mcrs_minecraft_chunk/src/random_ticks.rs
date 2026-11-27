use std::hash::Hash;

use crate::voxel_palette::VoxelPalette;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RandomTickPresence {
    pub block: bool,
    pub fluid: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionTickCounts {
    pub block_count: u16,
    pub fluid_count: u16,
}

const _: () = assert!(size_of::<SectionTickCounts>() == 4, "fits in 4 bytes");

impl SectionTickCounts {
    #[must_use]
    pub fn from_palette<V: Hash + Eq + Copy + Send + Sync + 'static, const DIM: usize>(
        palette: &VoxelPalette<V, DIM>,
        classify: impl FnMut(V) -> RandomTickPresence,
    ) -> Self {
        let mut counts = Self::default();
        counts.recount(palette, classify);
        counts
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.block_count == 0 && self.fluid_count == 0
    }

    pub fn fill(&mut self, presence: RandomTickPresence, volume: u16) {
        self.block_count = if presence.block { volume } else { 0 };
        self.fluid_count = if presence.fluid { volume } else { 0 };
    }

    pub fn recount<V: Hash + Eq + Copy + Send + Sync + 'static, const DIM: usize>(
        &mut self,
        palette: &VoxelPalette<V, DIM>,
        mut classify: impl FnMut(V) -> RandomTickPresence,
    ) {
        *self = Self::default();
        palette.for_each_count(|value, count| {
            let presence = classify(value);
            if presence.block {
                self.block_count = self.block_count.saturating_add(count);
            }
            if presence.fluid {
                self.fluid_count = self.fluid_count.saturating_add(count);
            }
        });
    }

    pub fn record_change(&mut self, previous: RandomTickPresence, next: RandomTickPresence) {
        self.block_count = self
            .block_count
            .saturating_add(u16::from(!previous.block && next.block))
            .saturating_sub(u16::from(previous.block && !next.block));
        self.fluid_count = self
            .fluid_count
            .saturating_add(u16::from(!previous.fluid && next.fluid))
            .saturating_sub(u16::from(previous.fluid && !next.fluid));
    }
}
