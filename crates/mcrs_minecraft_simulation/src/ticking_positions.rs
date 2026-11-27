use crate::random_tick::{RandomTickTable, RandomTicks};
use mcrs_minecraft_volume::{Palette, PaletteBuffer, SectionBlocks, SectionBlocksMut};
use mcrs_voxel::{BlockStateId, LocalPos, SectionPos};

fn adjust_kinds(block_count: &mut u16, fluid_count: &mut u16, old: RandomTicks, new: RandomTicks) {
    let step = |count: &mut u16, flag: RandomTicks| match (old.contains(flag), new.contains(flag)) {
        (false, true) => *count = count.saturating_add(1),
        (true, false) => *count = count.saturating_sub(1),
        (false, false) | (true, true) => {}
    };
    step(block_count, RandomTicks::BLOCK);
    step(fluid_count, RandomTicks::FLUID);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TickingPositions {
    positions: Vec<u16>,
    block_count: u16,
    fluid_count: u16,
}

impl TickingPositions {
    pub fn clear(&mut self) {
        self.positions.clear();
        self.block_count = 0;
        self.fluid_count = 0;
    }

    pub fn fill_uniform(&mut self, state: BlockStateId, table: &RandomTickTable) {
        self.clear();
        let kinds = table.kinds(state);
        if kinds.is_empty() {
            return;
        }
        let Ok(volume) = u16::try_from(SectionPos::VOLUME) else {
            return;
        };
        let count = |flag| if kinds.contains(flag) { volume } else { 0 };
        self.block_count = count(RandomTicks::BLOCK);
        self.fluid_count = count(RandomTicks::FLUID);
        self.positions.reserve(SectionPos::VOLUME);
        for index in 0..SectionPos::VOLUME {
            self.positions
                .push(local_index(LocalPos::from_index(index)));
        }
    }

    pub fn rebuild<S: SectionBlocks + ?Sized>(
        &mut self,
        section: &S,
        table: &RandomTickTable,
        palette: &mut PaletteBuffer,
    ) {
        self.clear();
        let mut total = 0u16;
        for (state, count) in section.palette(palette).iter() {
            let kinds = table.kinds(state);
            if kinds.is_empty() {
                continue;
            }
            if kinds.contains(RandomTicks::BLOCK) {
                self.block_count = self.block_count.saturating_add(count);
            }
            if kinds.contains(RandomTicks::FLUID) {
                self.fluid_count = self.fluid_count.saturating_add(count);
            }
            total = total.saturating_add(count);
        }
        if total == 0 {
            return;
        }
        self.positions.reserve(usize::from(total));
        section.for_each(|at, state| {
            if table.ticks(state) {
                self.positions.push(local_index(at));
            }
        });
    }

    pub fn record(
        &mut self,
        at: LocalPos,
        old: BlockStateId,
        new: BlockStateId,
        table: &RandomTickTable,
    ) {
        let (old_kinds, new_kinds) = (table.kinds(old), table.kinds(new));
        let (blocks, fluids) = (&mut self.block_count, &mut self.fluid_count);
        adjust_kinds(blocks, fluids, old_kinds, new_kinds);
        let found = self.positions.binary_search(&local_index(at));
        match (!old_kinds.is_empty(), !new_kinds.is_empty(), found) {
            (false, true, Err(slot)) => self.positions.insert(slot, local_index(at)),
            (true, false, Ok(slot)) => {
                self.positions.remove(slot);
            }
            _ => {}
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u16] {
        &self.positions
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    #[must_use]
    pub fn block_count(&self) -> u16 {
        self.block_count
    }

    #[must_use]
    pub fn fluid_count(&self) -> u16 {
        self.fluid_count
    }
}

pub(crate) fn local_index(at: LocalPos) -> u16 {
    at.index() as u16
}

#[derive(Debug)]
pub struct TickingSectionMut<'a, S> {
    section: S,
    tracker: Option<Tracker<'a>>,
}

#[derive(Debug)]
struct Tracker<'a> {
    ticking: &'a mut TickingPositions,
    table: &'a RandomTickTable,
}

impl<'a, S: SectionBlocksMut> TickingSectionMut<'a, S> {
    pub fn new(section: S, ticking: &'a mut TickingPositions, table: &'a RandomTickTable) -> Self {
        Self {
            section,
            tracker: Some(Tracker { ticking, table }),
        }
    }

    pub fn untracked(section: S) -> Self {
        Self {
            section,
            tracker: None,
        }
    }

    fn rebuild(&mut self) {
        if let Some(tracker) = self.tracker.as_mut() {
            tracker
                .ticking
                .rebuild(&self.section, tracker.table, &mut PaletteBuffer::default());
        }
    }
}

impl<S: SectionBlocksMut> SectionBlocks for TickingSectionMut<'_, S> {
    #[inline]
    fn get(&self, at: LocalPos) -> BlockStateId {
        self.section.get(at)
    }

    fn palette<'s>(&'s self, buffer: &'s mut PaletteBuffer) -> Palette<'s> {
        self.section.palette(buffer)
    }
}

impl<S: SectionBlocksMut> SectionBlocksMut for TickingSectionMut<'_, S> {
    fn set(&mut self, at: LocalPos, state: BlockStateId) -> BlockStateId {
        let old = self.section.set(at, state);
        if let Some(tracker) = self.tracker.as_mut() {
            tracker.ticking.record(at, old, state, tracker.table);
        }
        old
    }

    fn fill(&mut self, state: BlockStateId) {
        self.section.fill(state);
        if let Some(tracker) = self.tracker.as_mut() {
            tracker.ticking.fill_uniform(state, tracker.table);
        }
    }

    fn fill_box(&mut self, min: LocalPos, max: LocalPos, state: BlockStateId) {
        self.section.fill_box(min, max, state);
        self.rebuild();
    }
}
