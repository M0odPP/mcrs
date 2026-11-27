use bevy_ecs::prelude::Resource;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_resource::Id;

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvRegistryTables {
    pub(crate) rain_biomes: Box<[bool]>,
    pub(crate) freeze_immune_items: Box<[bool]>,
    pub(crate) item_count: usize,
}

const _: () = {
    assert!(
        size_of::<EnvRegistryTables>() == 40,
        "EnvRegistryTables size must match two boxed slices plus item count"
    );
    assert!(
        align_of::<EnvRegistryTables>() == 8,
        "EnvRegistryTables alignment must match pointer alignment"
    );
};

impl EnvRegistryTables {
    #[must_use]
    pub fn is_rain_biome(&self, biome_index: usize) -> bool {
        self.rain_biomes.get(biome_index).copied().unwrap_or(false)
    }

    #[must_use]
    pub fn is_freeze_immune_wire(&self, wire_id: u16) -> bool {
        Id::<Item>::checked(wire_id, self.item_count).is_some_and(|item_id| {
            self.freeze_immune_items
                .get(item_id.index())
                .copied()
                .unwrap_or(false)
        })
    }
}
