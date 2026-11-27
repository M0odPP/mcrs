use crate::container_facing::ContainerFacing;
use crate::container_kind::ContainerKind;
use mcrs_voxel::BlockPos;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObstructionRule {
    None,
    BlockAbove,
    FacingNeighbor,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Clearance {
    Clear,
    Blocked,
}

#[must_use]
pub const fn obstruction_rule(kind: ContainerKind) -> ObstructionRule {
    use ContainerKind::{Chest, CopperChest, EnderChest, ShulkerBox, TrappedChest};
    match kind {
        Chest | TrappedChest | CopperChest(_) | EnderChest => ObstructionRule::BlockAbove,
        ShulkerBox => ObstructionRule::FacingNeighbor,
        _ => ObstructionRule::None,
    }
}

#[must_use]
pub fn obstructing_pos(
    pos: BlockPos,
    kind: ContainerKind,
    face: ContainerFacing,
) -> Option<BlockPos> {
    match obstruction_rule(kind) {
        ObstructionRule::None => None,
        ObstructionRule::BlockAbove => ContainerFacing::Up.offset_pos(pos),
        ObstructionRule::FacingNeighbor => face.offset_pos(pos),
    }
}

#[must_use]
pub const fn can_open_single(rule: ObstructionRule, neighbor: Clearance) -> bool {
    matches!(rule, ObstructionRule::None) || matches!(neighbor, Clearance::Clear)
}

#[must_use]
pub const fn can_open_double(first: Clearance, second: Clearance) -> bool {
    matches!(first, Clearance::Clear) && matches!(second, Clearance::Clear)
}
