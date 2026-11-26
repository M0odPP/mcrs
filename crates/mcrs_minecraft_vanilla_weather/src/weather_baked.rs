use std::mem::{align_of, size_of};

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::prelude::{Res, ResMut, Resource};
use mcrs_minecraft_biome_file::BiomeFile;
use mcrs_minecraft_block::block_keys::Block;
use mcrs_minecraft_block::block_tag_keys::LIGHTNING_RODS;
use mcrs_minecraft_block::definition::Blocks;
use mcrs_minecraft_entity::damage_type_keys;
use mcrs_minecraft_entity::registry_keys::DAMAGE_TYPE;
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_voxel::BlockStateId;

#[derive(Resource, PartialEq, Eq)]
pub(crate) struct WeatherBaked {
    rods: Vec<bool>,
    rain_biomes: Vec<bool>,
    damage_type: Option<u32>,
    fire: BlockStateId,
}

const _: () = assert!(
    size_of::<WeatherBaked>() == 64,
    "WeatherBaked size must match two Vec<bool> tables plus id fields"
);
const _: () = assert!(
    align_of::<WeatherBaked>() == 8,
    "WeatherBaked alignment must match pointer alignment"
);

impl Default for WeatherBaked {
    fn default() -> Self {
        Self {
            rods: Vec::new(),
            rain_biomes: Vec::new(),
            damage_type: None,
            fire: BlockStateId::AIR,
        }
    }
}

impl WeatherBaked {
    pub(crate) fn is_unbaked(&self) -> bool {
        self.rain_biomes.is_empty()
    }

    pub(crate) fn is_rod(&self, state: BlockStateId) -> bool {
        self.rods.get(state.index()).copied().unwrap_or(false)
    }

    pub(crate) fn can_rain(&self, biome_index: usize) -> bool {
        !self.rain_biomes.is_empty() && self.rain_biomes.get(biome_index).copied().unwrap_or(false)
    }

    pub(crate) fn fire(&self) -> BlockStateId {
        self.fire
    }

    pub(crate) fn damage_type(&self) -> Option<u32> {
        self.damage_type
    }
}

pub(crate) fn bake_weather_tables(
    mut baked: ResMut<WeatherBaked>,
    blocks: Res<Blocks>,
    registries: Res<CurrentRegistries>,
) {
    let mut rods = vec![false; blocks.state_count()];
    if let Some(tags) = registries.tags::<Block>()
        && let Some(tag) = tags.id(LIGHTNING_RODS)
    {
        for id in blocks.states() {
            if let Some(block) = blocks.block_index(id)
                && tags.contains(tag, block)
                && let Some(slot) = rods.get_mut(id.index())
            {
                *slot = true;
            }
        }
    }
    let rain_biomes = registries
        .registry::<BiomeFile>()
        .map_or_else(Vec::new, |biomes| {
            biomes
                .iter()
                .map(|(_, _, biome)| {
                    biome.has_precipitation
                        && mcrs_minecraft_weather::warm_enough_for_rain(biome.temperature)
                })
                .collect()
        });
    baked.set_if_neq(WeatherBaked {
        rods,
        rain_biomes,
        damage_type: registries
            .id_of(
                DAMAGE_TYPE.location(),
                damage_type_keys::LIGHTNING_BOLT.location(),
            )
            .map(u32::from),
        fire: blocks.default_state_of(Block::Fire.id()),
    });
}
