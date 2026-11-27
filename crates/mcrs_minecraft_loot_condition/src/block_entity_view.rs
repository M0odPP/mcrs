use mcrs_minecraft_predicate::{BlockHost, ItemSubject};

pub trait BlockEntityView {
    fn block_entity_slot(&self, _slot: u16) -> Option<ItemSubject<'_>> {
        None
    }
}

impl<F, E> BlockEntityView for BlockHost<F, E> {}
