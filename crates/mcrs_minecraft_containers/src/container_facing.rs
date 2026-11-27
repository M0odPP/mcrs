use mcrs_voxel::BlockPos;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContainerFacing {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl ContainerFacing {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "down" => Some(Self::Down),
            "up" => Some(Self::Up),
            "north" => Some(Self::North),
            "south" => Some(Self::South),
            "west" => Some(Self::West),
            "east" => Some(Self::East),
            _ => None,
        }
    }

    #[must_use]
    pub const fn offset(self) -> BlockPos {
        match self {
            Self::Down => BlockPos::new(0, -1, 0),
            Self::Up => BlockPos::new(0, 1, 0),
            Self::North => BlockPos::new(0, 0, -1),
            Self::South => BlockPos::new(0, 0, 1),
            Self::West => BlockPos::new(-1, 0, 0),
            Self::East => BlockPos::new(1, 0, 0),
        }
    }

    #[must_use]
    pub fn offset_pos(self, pos: BlockPos) -> Option<BlockPos> {
        let delta = self.offset();
        let next_x = pos.x.checked_add(delta.x)?;
        let next_y = pos.y.checked_add(delta.y)?;
        let next_z = pos.z.checked_add(delta.z)?;
        Some(BlockPos::new(next_x, next_y, next_z))
    }

    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::North | Self::South | Self::West | Self::East)
    }
}
