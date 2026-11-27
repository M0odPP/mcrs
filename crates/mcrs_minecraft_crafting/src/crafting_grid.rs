use crate::craft_output::CraftOutput;
use crate::crafting_rules::{CraftingRules, ItemId};
use crate::grid_slot::GridSlot;
use crate::ingredient_ref::IngredientRef;
use crate::shaped_recipe::{MirrorMode, ShapedRecipe};
use crate::shapeless_recipe::ShapelessRecipe;
use crate::transmute_recipe::{TransmuteCountMode, TransmuteRecipe, TransmuteResult};
use core::mem::{align_of, size_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridSize {
    TwoByTwo = 2,
    ThreeByThree = 3,
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CraftingGrid {
    pub size: GridSize,
    pub slots: [Option<GridSlot>; 9],
}

const _: () = assert!(
    size_of::<GridSize>() == 1
        && align_of::<GridSize>() == 1
        && size_of::<CraftingGrid>() == 184
        && align_of::<CraftingGrid>() == 4,
    "GridSize and CraftingGrid layout"
);

impl CraftingGrid {
    #[must_use]
    pub const fn new(size: GridSize) -> Self {
        Self {
            size,
            slots: [None; 9],
        }
    }

    const fn cell_count(&self) -> usize {
        (self.size as usize) * (self.size as usize)
    }

    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn iter_non_empty(&self) -> impl Iterator<Item = (usize, GridSlot)> + '_ {
        let slice = self.slots.iter().take(self.cell_count()).enumerate();
        slice.filter_map(|(index, slot)| slot.map(|stack| (index, stack)))
    }

    #[must_use]
    pub fn max_crafts(&self) -> u8 {
        self.iter_non_empty()
            .map(|(_, stack)| stack.count)
            .min()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn consume_once(&mut self, rules: &impl CraftingRules) -> [Option<ItemId>; 9] {
        let limit = self.cell_count();
        let (mut overflow, mut overflow_index) = ([None; 9], 0usize);
        for slot_ref in self.slots.iter_mut().take(limit) {
            let Some(mut stack) = *slot_ref else { continue };
            let remainder = rules.crafting_remainder(stack.item);
            if stack.count <= 1 {
                *slot_ref = remainder;
                continue;
            }
            stack.count -= 1;
            *slot_ref = Some(stack);
            if let (Some(left), Some(out)) = (remainder, overflow.get_mut(overflow_index)) {
                *out = Some(left.item);
                overflow_index += 1;
            }
        }
        overflow
    }

    #[must_use]
    pub fn match_shaped(
        &self,
        recipe: &ShapedRecipe<'_>,
        rules: &impl CraftingRules,
    ) -> Option<CraftOutput> {
        let (width, height) = (usize::from(recipe.width), usize::from(recipe.height));
        let cells = width
            .checked_mul(height)
            .filter(|&total| total > 0 && self.max_crafts() > 0)
            .and_then(|total| recipe.pattern.get(..total))?;
        let (mut min_x, mut max_x) = (width, 0);
        let (mut min_y, mut max_y) = (height, 0);
        let mut occupied = 0usize;
        for (index, cell) in cells.iter().enumerate() {
            if cell.is_some() {
                (min_x, max_x) = (min_x.min(index % width), max_x.max(index % width));
                (min_y, max_y) = (min_y.min(index / width), max_y.max(index / width));
                occupied += 1;
            }
        }
        let side = self.size as usize;
        let span_x = max_x.saturating_sub(min_x) + 1;
        let span_y = max_y.saturating_sub(min_y) + 1;
        let invalid = occupied == 0 || span_x > side || span_y > side;
        if invalid || self.iter_non_empty().count() != occupied {
            return None;
        }
        let flips = usize::from(recipe.mirror == MirrorMode::AllowHorizontal) + 1;
        let stride = side - span_x + 1;
        let fits = |offset: usize, flip: bool| -> bool {
            (0..(span_x * span_y)).all(|cell| {
                let (x, y) = (cell % span_x, cell / span_x);
                let pattern_x = min_x + if flip { span_x - 1 - x } else { x };
                let pattern_index = (min_y + y) * width + pattern_x;
                let need = cells.get(pattern_index).copied().flatten();
                let cell_index = (offset / stride + y) * side + offset % stride + x;
                let slot = self.slots.get(cell_index).copied().flatten();
                need.is_none_or(|wanted| {
                    slot.is_some_and(|stack| wanted.matches(stack.item, rules))
                })
            })
        };
        (0..((side - span_y + 1) * stride))
            .any(|offset| (0..flips).any(|flip| fits(offset, flip == 1)))
            .then_some(CraftOutput::Fixed(recipe.result_item, recipe.result_count))
    }

    #[must_use]
    pub fn match_shapeless(
        &self,
        recipe: &ShapelessRecipe<'_>,
        rules: &impl CraftingRules,
    ) -> Option<CraftOutput> {
        let count = recipe.ingredients.len();
        let exact = count > 0 && self.max_crafts() > 0 && self.iter_non_empty().count() == count;
        (exact && assign_shapeless(0, 0, recipe.ingredients, self, rules))
            .then_some(CraftOutput::Fixed(recipe.result_item, recipe.result_count))
    }

    #[must_use]
    pub fn match_transmute(
        &self,
        recipe: &TransmuteRecipe<'_>,
        rules: &impl CraftingRules,
    ) -> Option<CraftOutput> {
        let (mut source, mut material_count) = (None, 0u8);
        for (index, stack) in self.iter_non_empty() {
            if stack.count == 0 {
                return None;
            }
            if recipe.input.matches(stack.item, rules) && source.is_none() {
                source = Some((u8::try_from(index).ok()?, stack.item));
            } else if recipe.material.matches(stack.item, rules) {
                material_count = material_count.saturating_add(1);
            } else {
                return None;
            }
        }
        let (source_slot, input_item) = source?;
        if material_count < recipe.material_min || material_count > recipe.material_max {
            return None;
        }
        let item = match recipe.result_item {
            TransmuteResult::PreserveInput => input_item,
            TransmuteResult::Fixed(fixed) => fixed,
        };
        let count = match recipe.count_mode {
            TransmuteCountMode::FixedResult => recipe.result_count,
            TransmuteCountMode::AddMaterialCount => recipe
                .result_count
                .saturating_add(material_count)
                .min(rules.max_stack_size(item).max(1)),
        };
        Some(CraftOutput::Transmute {
            item,
            count,
            source_slot,
        })
    }

    #[must_use]
    pub fn match_armor_dye(&self, rules: &impl CraftingRules) -> Option<CraftOutput> {
        let (mut armor, mut dye_count) = (None, 0u8);
        let (mut max_sum, mut color_count) = (0u32, 0u32);
        let mut sums = [0u32; 3];
        for (index, stack) in self.iter_non_empty() {
            if stack.count == 0 {
                return None;
            }
            let rgb = if rules.is_dyeable_armor(stack.item) && armor.is_none() {
                armor = Some((u8::try_from(index).ok()?, stack.item));
                stack.dyed_color
            } else {
                dye_count = dye_count.saturating_add(1);
                Some(rules.dye_rgb(stack.item)?)
            };
            if let Some(color) = rgb {
                let [red, green, blue] = [(color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF];
                let [sum_red, sum_green, sum_blue] = sums;
                sums = [sum_red + red, sum_green + green, sum_blue + blue];
                max_sum += red.max(green).max(blue);
                color_count += 1;
            }
        }
        let (source_slot, item) = armor?;
        if dye_count == 0 || color_count == 0 {
            return None;
        }
        let averages = sums.map(|sum| sum / color_count);
        let max_average = averages.into_iter().max().unwrap_or(0);
        let mut rgb = 0u32;
        if max_average > 0 {
            let scale = (max_sum as f32 / color_count as f32) / max_average as f32;
            let [red, green, blue] = averages.map(|value| ((value as f32 * scale) as u32).min(255));
            rgb = (red << 16) | (green << 8) | blue;
        }
        Some(CraftOutput::DyedArmor {
            item,
            rgb,
            source_slot,
        })
    }

    #[must_use]
    pub fn match_repair_item(&self) -> Option<CraftOutput> {
        let (mut first, mut second) = (None, None);
        for (_, stack) in self.iter_non_empty() {
            if stack.max_damage == 0 || stack.max_stack_size != 1 || stack.count != 1 {
                return None;
            }
            match (first, second) {
                (None, _) => first = Some(stack),
                (_, None) => second = Some(stack),
                _ => return None,
            }
        }
        let (first_stack, second_stack) = (first?, second?);
        if first_stack.item != second_stack.item {
            return None;
        }
        let limit = first_stack.max_damage;
        let restored = limit
            .saturating_sub(first_stack.damage)
            .saturating_add(limit.saturating_sub(second_stack.damage))
            .saturating_add(limit / 20);
        Some(CraftOutput::RepairedTool(
            first_stack.item,
            limit.saturating_sub(restored),
        ))
    }
}

fn assign_shapeless(
    index: usize,
    mask: u16,
    list: &[IngredientRef<'_>],
    grid: &CraftingGrid,
    rules: &impl CraftingRules,
) -> bool {
    let Some(ingredient) = list.get(index) else {
        return index == list.len();
    };
    for slot in 0..grid.cell_count() {
        let bit = 1u16 << slot;
        let stack = grid.slots.get(slot).copied().flatten();
        if (mask & bit) == 0
            && stack.is_some_and(|entry| ingredient.matches(entry.item, rules))
            && assign_shapeless(index + 1, mask | bit, list, grid, rules)
        {
            return true;
        }
    }
    false
}
