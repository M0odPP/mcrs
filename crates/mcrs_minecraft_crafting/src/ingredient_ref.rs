use crate::crafting_rules::{CraftingRules, ItemId, TagId};
use core::mem::{align_of, size_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IngredientRef<'a> {
    Item(ItemId),
    Tag(TagId),
    Items(&'a [ItemId]),
}

const _: () = assert!(
    size_of::<IngredientRef<'static>>() == size_of::<usize>() * 3
        && align_of::<IngredientRef<'static>>() == align_of::<usize>(),
    "IngredientRef size and align"
);

impl IngredientRef<'_> {
    #[must_use]
    pub fn matches(&self, item: ItemId, rules: &impl CraftingRules) -> bool {
        match *self {
            Self::Item(id) => item == id,
            Self::Tag(tag) => rules.tag_contains(tag, item),
            Self::Items(ids) => ids.contains(&item),
        }
    }
}
