use crate::container_kind::{ContainerKind, is_shulker_box_id};

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InsertedItem {
    Standard,
    ShulkerBox,
}

#[must_use]
pub const fn can_insert_into_container(container: ContainerKind, item: InsertedItem) -> bool {
    let shulker_target = matches!(container, ContainerKind::ShulkerBox);
    !(shulker_target && matches!(item, InsertedItem::ShulkerBox))
}

#[must_use]
pub fn can_insert_item_id(container: ContainerKind, item_id: &str) -> bool {
    let item = if is_shulker_box_id(item_id) {
        InsertedItem::ShulkerBox
    } else {
        InsertedItem::Standard
    };
    can_insert_into_container(container, item)
}
