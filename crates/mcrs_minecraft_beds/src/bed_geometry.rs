use core::mem::{align_of, size_of};
use mcrs_voxel::BlockPos;

pub const BED_TOP_OFFSET: f64 = 0.5625;
pub const STRAW_BED_TOP_OFFSET: f64 = 0.25;
pub const SLEEP_ANCHOR_CLEARANCE: f64 = 0.125;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BedFacing {
    North,
    South,
    West,
    East,
}

impl BedFacing {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    #[must_use]
    pub const fn step_toward_head(self, pos: BlockPos) -> BlockPos {
        match self {
            Self::North => pos.offset(0, 0, -1),
            Self::South => pos.offset(0, 0, 1),
            Self::West => pos.offset(-1, 0, 0),
            Self::East => pos.offset(1, 0, 0),
        }
    }

    #[must_use]
    pub const fn step_toward_foot(self, pos: BlockPos) -> BlockPos {
        self.opposite().step_toward_head(pos)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BedPart {
    Foot,
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BedState {
    pub facing: BedFacing,
    pub part: BedPart,
    pub occupied: bool,
}

impl BedState {
    #[must_use]
    pub const fn new(facing: BedFacing, part: BedPart) -> Self {
        Self {
            facing,
            part,
            occupied: false,
        }
    }

    #[must_use]
    pub const fn occupy(self) -> Self {
        Self {
            occupied: true,
            ..self
        }
    }

    #[must_use]
    pub const fn vacate(self) -> Self {
        Self {
            occupied: false,
            ..self
        }
    }

    #[must_use]
    pub const fn partner_pos(self, pos: BlockPos) -> BlockPos {
        match self.part {
            BedPart::Foot => self.facing.step_toward_head(pos),
            BedPart::Head => self.facing.step_toward_foot(pos),
        }
    }

    #[must_use]
    pub fn is_expected_partner(self, partner: Self) -> bool {
        self.facing == partner.facing && self.part != partner.part
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BedPair {
    pub foot: BlockPos,
    pub head: BlockPos,
    pub facing: BedFacing,
}

impl BedPair {
    #[must_use]
    pub const fn from_placement(foot: BlockPos, facing: BedFacing) -> Self {
        let head = facing.step_toward_head(foot);
        Self { foot, head, facing }
    }

    #[must_use]
    pub const fn from_block(pos: BlockPos, state: BedState) -> Self {
        let facing = state.facing;
        match state.part {
            BedPart::Foot => Self::from_placement(pos, facing),
            BedPart::Head => Self {
                foot: facing.step_toward_foot(pos),
                head: pos,
                facing,
            },
        }
    }
}

#[must_use]
pub fn explosion_origin(head: BlockPos) -> WorldPoint {
    WorldPoint {
        x: f64::from(head.x) + 0.5,
        y: f64::from(head.y) + 0.5,
        z: f64::from(head.z) + 0.5,
    }
}

#[must_use]
pub fn sleep_anchor(head: BlockPos, bed_top_offset: f64) -> WorldPoint {
    let origin = explosion_origin(head);
    WorldPoint {
        y: f64::from(head.y) + bed_top_offset + SLEEP_ANCHOR_CLEARANCE,
        ..origin
    }
}

const _: () = {
    const fn layout<T>(size: usize, align: usize) -> bool {
        size_of::<T>() == size && align_of::<T>() == align
    }
    assert!(layout::<BedState>(3, 1), "BedState layout");
    assert!(layout::<BedPair>(28, 4), "BedPair layout");
    assert!(layout::<WorldPoint>(24, 8), "WorldPoint layout");
};
