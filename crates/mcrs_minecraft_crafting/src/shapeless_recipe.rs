use crate::crafting_rules::ItemId;
use crate::ingredient_ref::IngredientRef;
use core::mem::{align_of, size_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapelessRecipe<'a> {
    pub ingredients: &'a [IngredientRef<'a>],
    pub result_item: ItemId,
    pub result_count: u8,
}

const _: () = assert!(
    size_of::<ShapelessRecipe<'static>>() == size_of::<usize>() * 3
        && align_of::<ShapelessRecipe<'static>>() == align_of::<usize>(),
    "ShapelessRecipe size and align"
);
