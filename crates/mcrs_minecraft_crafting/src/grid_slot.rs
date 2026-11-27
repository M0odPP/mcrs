use crate::crafting_rules::ItemId;
use core::mem::{align_of, size_of};

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSlot {
    pub item: ItemId,
    pub count: u8,
    pub max_stack_size: u8,
    pub damage: u32,
    pub max_damage: u32,
    pub dyed_color: Option<u32>,
}

const _: () = assert!(
    size_of::<GridSlot>() == 20 && align_of::<GridSlot>() == 4,
    "GridSlot size and align"
);

impl GridSlot {
    #[must_use]
    pub const fn simple(item: ItemId, count: u8, max_stack_size: u8) -> Self {
        Self {
            item,
            count,
            max_stack_size,
            damage: 0,
            max_damage: 0,
            dyed_color: None,
        }
    }

    #[must_use]
    pub const fn with_damage(mut self, damage: u32, max_damage: u32) -> Self {
        self.max_stack_size = 1;
        self.damage = damage;
        self.max_damage = max_damage;
        self
    }

    #[must_use]
    pub const fn with_dyed_color(mut self, dyed_color: Option<u32>) -> Self {
        self.max_stack_size = 1;
        self.dyed_color = dyed_color;
        self
    }
}
