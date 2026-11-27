use crate::crafting_rules::ItemId;
use crate::ingredient_ref::IngredientRef;
use core::mem::{align_of, size_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MirrorMode {
    ExactOnly,
    AllowHorizontal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapedRecipe<'a> {
    pub width: u8,
    pub height: u8,
    pub pattern: &'a [Option<IngredientRef<'a>>],
    pub mirror: MirrorMode,
    pub result_item: ItemId,
    pub result_count: u8,
}

const _: () = assert!(
    size_of::<MirrorMode>() == 1 && align_of::<MirrorMode>() == 1,
    "MirrorMode size and align"
);
const _: () = assert!(
    size_of::<ShapedRecipe<'static>>() == size_of::<usize>() * 2 + 8
        && align_of::<ShapedRecipe<'static>>() == align_of::<usize>(),
    "ShapedRecipe size and align"
);
