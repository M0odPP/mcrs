use mcrs_minecraft_crafting::{
    CraftOutput, CraftingGrid, CraftingRules, GridSize, GridSlot, IngredientRef, ItemId,
    MirrorMode, ShapedRecipe, ShapelessRecipe, TagId, TransmuteCountMode, TransmuteRecipe,
    TransmuteResult,
};
use mcrs_test_alloc::{CountingAllocator, allocations};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

const PLANK: Option<IngredientRef<'static>> = Some(IngredientRef::Tag(TagId(1)));
const STICK: &[Option<IngredientRef<'static>>] = &[PLANK, None, PLANK, None];
const SHELF: &[Option<IngredientRef<'static>>] =
    &[PLANK, PLANK, PLANK, None, None, None, PLANK, PLANK, PLANK];
const STAIRS: &[Option<IngredientRef<'static>>] =
    &[PLANK, None, None, PLANK, PLANK, None, PLANK, PLANK, PLANK];

struct TestRules;

impl CraftingRules for TestRules {
    fn tag_contains(&self, tag: TagId, item: ItemId) -> bool {
        tag == TagId(1) && (item == ItemId(2) || item == ItemId(3))
    }
    fn crafting_remainder(&self, item: ItemId) -> Option<GridSlot> {
        (item == ItemId(11)).then_some(GridSlot::simple(ItemId(12), 1, 16))
    }
    fn dye_rgb(&self, item: ItemId) -> Option<u32> {
        (item == ItemId(8)).then_some(11_546_150)
    }
    fn is_dyeable_armor(&self, item: ItemId) -> bool {
        item == ItemId(9)
    }
    fn max_stack_size(&self, item: ItemId) -> u8 {
        if item == ItemId(19) { 2 } else { 64 }
    }
}

fn grid(size: GridSize, items: &[u16], counts: &[(usize, u8)]) -> CraftingGrid {
    let mut output = CraftingGrid::new(size);
    for (slot, &item) in output.slots.iter_mut().zip(items) {
        *slot = (item != 0).then_some(GridSlot::simple(ItemId(item), 1, 64));
    }
    for &(index, count) in counts {
        if let Some(Some(slot)) = output.slots.get_mut(index) {
            slot.count = count;
        }
    }
    output
}

const fn fixed(id: u16, count: u8) -> Option<CraftOutput> {
    Some(CraftOutput::Fixed(ItemId(id), count))
}

fn slot0_value(result: Option<CraftOutput>) -> Option<(u16, u32)> {
    match result? {
        CraftOutput::Transmute {
            item,
            count,
            source_slot: 0,
        } => Some((item.0, u32::from(count))),
        CraftOutput::DyedArmor {
            item,
            rgb,
            source_slot: 0,
        } => Some((item.0, rgb)),
        CraftOutput::RepairedTool(item, damage) => Some((item.0, damage)),
        _ => None,
    }
}

#[test]
fn crafting_zero_alloc() {
    let rules = TestRules;
    let packed = grid(GridSize::ThreeByThree, &[2, 2, 2, 2, 2, 2], &[]);
    let stick_grid = grid(GridSize::TwoByTwo, &[0, 2, 0, 3], &[(1, 3), (3, 2)]);
    let stairs_grid = grid(GridSize::ThreeByThree, &[0, 0, 2, 0, 2, 2, 2, 2, 2], &[]);
    let planks_grid = grid(GridSize::TwoByTwo, &[1, 0, 0, 2], &[(0, 2)]);
    let shulker_grid = grid(GridSize::ThreeByThree, &[6, 8], &[]);
    let map_grid = grid(GridSize::ThreeByThree, &[18, 8, 8], &[]);
    let mut remainders = grid(GridSize::ThreeByThree, &[11, 11], &[(0, 2)]);
    let mut dye_grid = grid(GridSize::ThreeByThree, &[9, 8], &[]);
    for slot in dye_grid.slots.get_mut(0).into_iter().flatten() {
        *slot = slot.with_dyed_color(Some(11_546_150));
    }
    let mut repair = grid(GridSize::ThreeByThree, &[10, 0, 0, 0, 10], &[]);
    for slot in repair.slots.iter_mut().flatten() {
        *slot = slot.with_damage(200, 250);
    }
    let mut recipe = ShapedRecipe {
        width: 3,
        height: 3,
        pattern: SHELF,
        mirror: MirrorMode::ExactOnly,
        result_item: ItemId(20),
        result_count: 1,
    };
    let planks = ShapelessRecipe {
        ingredients: &[IngredientRef::Item(ItemId(1)), IngredientRef::Tag(TagId(1))],
        result_item: ItemId(2),
        result_count: 4,
    };
    let mut transmute = TransmuteRecipe {
        input: IngredientRef::Item(ItemId(6)),
        material: IngredientRef::Item(ItemId(8)),
        material_min: 1,
        material_max: 1,
        result_item: TransmuteResult::Fixed(ItemId(7)),
        result_count: 1,
        count_mode: TransmuteCountMode::FixedResult,
    };
    let before = allocations();
    let gap = packed.match_shaped(&recipe, &rules);
    let empty = CraftingGrid::new(GridSize::TwoByTwo).match_shaped(&recipe, &rules);
    (recipe.width, recipe.height) = (2, 2);
    recipe.pattern = STICK;
    (recipe.result_item, recipe.result_count) = (ItemId(4), 4);
    let stick = stick_grid.match_shaped(&recipe, &rules);
    let max_crafts = stick_grid.max_crafts();
    (recipe.width, recipe.height) = (3, 3);
    recipe.pattern = STAIRS;
    (recipe.mirror, recipe.result_item) = (MirrorMode::AllowHorizontal, ItemId(5));
    let stair = stairs_grid.match_shaped(&recipe, &rules);
    let plank = planks_grid.match_shapeless(&planks, &rules);
    let shulker = shulker_grid.match_transmute(&transmute, &rules);
    transmute.input = IngredientRef::Item(ItemId(18));
    (transmute.material_max, transmute.result_item) = (8, TransmuteResult::PreserveInput);
    transmute.count_mode = TransmuteCountMode::AddMaterialCount;
    let map = map_grid.match_transmute(&transmute, &rules);
    transmute.result_item = TransmuteResult::Fixed(ItemId(19));
    let capped = map_grid.match_transmute(&transmute, &rules);
    let overflow = remainders.consume_once(&rules);
    let (dye, repaired) = (dye_grid.match_armor_dye(&rules), repair.match_repair_item());
    assert_eq!(allocations() - before, 0, "crafting uses zero allocations");
    assert_eq!(empty, None, "empty grid matches nothing");
    assert_eq!(gap, None, "missing middle row must reject");
    assert_eq!(max_crafts, 2, "max crafts equals minimum count");
    assert_eq!(stick, fixed(4, 4), "shifted stick recipe matches");
    assert_eq!(stair, fixed(5, 4), "mirrored stairs recipe matches");
    assert_eq!(plank, fixed(2, 4), "shapeless planks recipe matches");
    assert_eq!(slot0_value(shulker), Some((7, 1)), "shulker swaps item");
    assert_eq!(slot0_value(map), Some((18, 3)), "transmute keeps input");
    assert_eq!(slot0_value(capped), Some((19, 2)), "transmute caps count");
    assert_eq!(slot0_value(dye), Some((9, 11_546_150)), "dye mixes rgb");
    assert_eq!(slot0_value(repaired), Some((10, 138)), "repair heals tool");
    let [first, second, ..] = remainders.slots;
    let [overflow_first, overflow_second, ..] = overflow;
    assert_eq!(first.map(|slot| slot.count), Some(1), "slot drops count");
    assert_eq!(second.map(|slot| slot.item.0), Some(12), "slot gets bucket");
    assert_eq!(overflow_first, Some(ItemId(12)), "extra overflows out");
    assert_eq!(overflow_second, None, "second overflow slot stays empty");
}
