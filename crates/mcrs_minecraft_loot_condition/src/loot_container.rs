use mcrs_minecraft_item_stack::ItemStack;
use mcrs_minecraft_predicate::{EntityView, ItemSubject};

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum LootContainer<'a, E> {
    Entity(E),
    Slots(&'a [Option<ItemStack>]),
}

impl<'a, E: Copy> LootContainer<'a, E> {
    #[must_use]
    pub fn slot<'w, W: EntityView<Entity = E> + ?Sized>(
        self,
        world: &'w W,
        slot: u16,
    ) -> Option<ItemSubject<'w>>
    where
        'a: 'w,
    {
        match self {
            Self::Entity(entity) => world.slot(entity, slot),
            Self::Slots(slots) => slots.get(usize::from(slot)).map(|item| match item {
                Some(stack) => ItemSubject::Stack(stack),
                None => ItemSubject::Empty,
            }),
        }
    }
}
