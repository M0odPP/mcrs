use crate::grid_slot::GridSlot;
use core::mem::{align_of, size_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagId(pub u16);

const _: () = assert!(
    size_of::<ItemId>() == 2 && align_of::<ItemId>() == 2,
    "ItemId size and align"
);
const _: () = assert!(
    size_of::<TagId>() == 2 && align_of::<TagId>() == 2,
    "TagId size and align"
);

pub trait CraftingRules {
    fn tag_contains(&self, tag: TagId, item: ItemId) -> bool;
    fn crafting_remainder(&self, item: ItemId) -> Option<GridSlot>;
    fn dye_rgb(&self, item: ItemId) -> Option<u32>;
    fn is_dyeable_armor(&self, item: ItemId) -> bool;
    fn max_stack_size(&self, item: ItemId) -> u8;
}
