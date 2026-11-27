use mcrs_minecraft_item_stack::ItemStack;
use mcrs_minecraft_predicate::PredicateScratch;

use crate::slot_source::SlotWrapper;
use crate::table::Candidate;

#[derive(Debug, Clone, Default)]
pub struct LootScratch {
    pub(crate) candidates: Vec<Candidate>,
    pub(crate) predicates: PredicateScratch,
    pub(crate) slot_wrappers: Vec<SlotWrapper>,
    pub(crate) rolled: Vec<Option<ItemStack>>,
    pub(crate) kept: Vec<ItemStack>,
    pub(crate) split: Vec<ItemStack>,
    pub(crate) free: Vec<usize>,
}

impl LootScratch {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}
