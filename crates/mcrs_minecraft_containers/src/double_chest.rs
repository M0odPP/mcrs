use crate::container_facing::ContainerFacing;
use crate::container_kind::ContainerKind;
use mcrs_voxel::BlockPos;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChestType {
    Single,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChestHalf {
    pub pos: BlockPos,
    pub kind: ContainerKind,
    pub facing: ContainerFacing,
    pub chest_type: ChestType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PairedChestTypes {
    pub placed: ChestType,
    pub neighbor: ChestType,
}

#[must_use]
pub fn partner_pos(pos: BlockPos, facing: ContainerFacing, side: ChestType) -> Option<BlockPos> {
    use ChestType::{Left, Right};
    use ContainerFacing::{East, North, South, West};
    let direction = match (facing, side) {
        (North, Left) | (South, Right) => Some(East),
        (North, Right) | (South, Left) => Some(West),
        (East, Left) | (West, Right) => Some(South),
        (East, Right) | (West, Left) => Some(North),
        _ => None,
    }?;
    direction.offset_pos(pos)
}

fn same_chest(kind: ContainerKind, facing: ContainerFacing, other: ChestHalf) -> bool {
    kind.supports_double_pairing()
        && kind == other.kind
        && facing.is_horizontal()
        && facing == other.facing
}

#[must_use]
pub fn can_pair_halves(first: ChestHalf, second: ChestHalf) -> bool {
    use ChestType::{Left, Right};
    let pair = (first.chest_type, second.chest_type);
    let opposite = matches!(pair, (Left, Right) | (Right, Left));
    let aligned = partner_pos(first.pos, first.facing, first.chest_type) == Some(second.pos);
    same_chest(first.kind, first.facing, second) && opposite && aligned
}

const fn paired(placed: ChestType, neighbor: ChestType) -> Option<PairedChestTypes> {
    Some(PairedChestTypes { placed, neighbor })
}

#[must_use]
pub fn pair_on_place(
    placed_pos: BlockPos,
    placed_kind: ContainerKind,
    facing: ContainerFacing,
    neighbor: ChestHalf,
) -> Option<PairedChestTypes> {
    if !same_chest(placed_kind, facing, neighbor) || neighbor.chest_type != ChestType::Single {
        return None;
    }
    if partner_pos(neighbor.pos, facing, ChestType::Left) == Some(placed_pos) {
        paired(ChestType::Right, ChestType::Left)
    } else if partner_pos(neighbor.pos, facing, ChestType::Right) == Some(placed_pos) {
        paired(ChestType::Left, ChestType::Right)
    } else {
        None
    }
}
