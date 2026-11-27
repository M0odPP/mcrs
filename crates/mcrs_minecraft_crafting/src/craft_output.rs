use crate::crafting_rules::ItemId;
use core::mem::{align_of, size_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CraftOutput {
    Fixed(ItemId, u8),
    Transmute {
        item: ItemId,
        count: u8,
        source_slot: u8,
    },
    DyedArmor {
        item: ItemId,
        rgb: u32,
        source_slot: u8,
    },
    RepairedTool(ItemId, u32),
}

const _: () = assert!(
    size_of::<CraftOutput>() == 8 && align_of::<CraftOutput>() == 4,
    "CraftOutput size and align"
);
