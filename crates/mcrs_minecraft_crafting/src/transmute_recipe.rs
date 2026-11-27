use crate::crafting_rules::ItemId;
use crate::ingredient_ref::IngredientRef;
use core::mem::{align_of, size_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransmuteCountMode {
    FixedResult,
    AddMaterialCount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransmuteResult {
    PreserveInput,
    Fixed(ItemId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransmuteRecipe<'a> {
    pub input: IngredientRef<'a>,
    pub material: IngredientRef<'a>,
    pub material_min: u8,
    pub material_max: u8,
    pub result_item: TransmuteResult,
    pub result_count: u8,
    pub count_mode: TransmuteCountMode,
}

const _: () = assert!(
    size_of::<TransmuteCountMode>() == 1 && align_of::<TransmuteCountMode>() == 1,
    "TransmuteCountMode size and align"
);
const _: () = assert!(
    size_of::<TransmuteResult>() == 4 && align_of::<TransmuteResult>() == 2,
    "TransmuteResult size and align"
);
const _: () = assert!(
    size_of::<TransmuteRecipe<'static>>() == size_of::<usize>() * 6 + 8
        && align_of::<TransmuteRecipe<'static>>() == align_of::<usize>(),
    "TransmuteRecipe size and align"
);
