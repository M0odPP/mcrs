use std::sync::Arc;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::prelude::{Res, ResMut, Resource};
use mcrs_minecraft_biome_file::BiomeFile;
use mcrs_minecraft_block::block_keys::Block;
use mcrs_minecraft_block::block_tag_keys;
use mcrs_minecraft_block::definition::{BlockDefinitions, BlockEntry, BlockSoundSets, Blocks};
use mcrs_minecraft_block::fluid_keys::Fluid;
use mcrs_minecraft_entity::damage_type::DamageType;
use mcrs_minecraft_entity::damage_type_keys;
use mcrs_minecraft_entity::registry_keys::DAMAGE_TYPE;
use mcrs_minecraft_env_damage::FallBlockKind;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_item_key::item_tag_keys;
use mcrs_minecraft_registries::{CurrentRegistries, Ref, Registry, Resolved, Wire};
use mcrs_minecraft_resource::{Id, ResourceKey};
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use mcrs_minecraft_sound::{BlockSoundSet, Sound};
use mcrs_minecraft_weather as weather;
use mcrs_voxel::BlockStateId;

use crate::env_state::SoundSpec;
use crate::registry_tables::EnvRegistryTables;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BakedEnvDamage {
    pub fall: Option<u32>,
    pub stalagmite: Option<u32>,
    pub in_wall: Option<u32>,
    pub cactus: Option<u32>,
    pub campfire: Option<u32>,
    pub sweet_berry_bush: Option<u32>,
    pub hot_floor: Option<u32>,
    pub out_of_world: Option<u32>,
    pub outside_border: Option<u32>,
    pub on_fire: Option<u32>,
    pub in_fire: Option<u32>,
    pub lava: Option<u32>,
    pub drown: Option<u32>,
    pub freeze: Option<u32>,
    pub player_small_fall: Option<Id<SoundEvent>>,
    pub player_big_fall: Option<Id<SoundEvent>>,
    pub generic_small_fall: Option<Id<SoundEvent>>,
    pub generic_big_fall: Option<Id<SoundEvent>>,
    pub honey_slide: Option<Id<SoundEvent>>,
    pub generic_burn: Option<Id<SoundEvent>>,
    pub extinguish_fire: Option<Id<SoundEvent>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnvFluidKind {
    #[default]
    None,
    Water(u8),
    Lava(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnvContactBlock {
    #[default]
    None,
    Cactus,
    CampfireLit,
    SoulCampfireLit,
    SweetBerryBush(u8),
    MagmaBlock,
    Fire,
    SoulFire,
    LavaCauldron,
    WaterCauldron(u8),
    PowderSnowCauldron(u8),
    PowderSnow,
    BubbleColumn,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FallSound {
    pub sound: Sound<Wire>,
    pub volume: f32,
    pub pitch: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvBlockEntry {
    pub fall_kind: FallBlockKind,
    pub contact: EnvContactBlock,
    pub fluid: EnvFluidKind,
    pub blocks_motion: bool,
    pub fall_sound: Option<FallSound>,
}

impl Default for EnvBlockEntry {
    fn default() -> Self {
        Self {
            fall_kind: FallBlockKind::Default,
            contact: EnvContactBlock::None,
            fluid: EnvFluidKind::None,
            blocks_motion: false,
            fall_sound: None,
        }
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct EnvBlockTable(Box<[EnvBlockEntry]>);

const _: () = {
    assert!(
        size_of::<BakedEnvDamage>() == 140,
        "BakedEnvDamage size must match cached damage and sound ids"
    );
    assert!(
        align_of::<BakedEnvDamage>() == 4,
        "BakedEnvDamage align must match u32 alignment"
    );
    assert!(
        size_of::<EnvFluidKind>() == 2,
        "EnvFluidKind size must match tag plus fluid level"
    );
    assert!(
        align_of::<EnvFluidKind>() == 1,
        "EnvFluidKind alignment must match u8 alignment"
    );
    assert!(
        size_of::<EnvContactBlock>() == 2,
        "EnvContactBlock size must match tag plus level or age"
    );
    assert!(
        align_of::<EnvContactBlock>() == 1,
        "EnvContactBlock alignment must match u8 alignment"
    );
    assert!(
        size_of::<FallSound>() == 24,
        "FallSound size must match sound ref plus floats"
    );
    assert!(
        align_of::<FallSound>() == 8,
        "FallSound align must match pointer alignment"
    );
    assert!(
        size_of::<EnvBlockEntry>() == 32,
        "EnvBlockEntry size must pack fluid and flags cleanly"
    );
    assert!(
        align_of::<EnvBlockEntry>() == 8,
        "EnvBlockEntry align must match pointer alignment"
    );
    assert!(
        size_of::<EnvBlockTable>() == 16,
        "EnvBlockTable size must match boxed slice fat pointer"
    );
    assert!(
        align_of::<EnvBlockTable>() == 8,
        "EnvBlockTable align must match pointer alignment"
    );
};

impl EnvBlockTable {
    #[must_use]
    pub fn get(&self, id: BlockStateId) -> Option<&EnvBlockEntry> {
        self.0.get(id.index())
    }
}

pub(crate) fn sound_spec_wire(
    sound: Id<SoundEvent>,
    volume: f32,
    pitch: f32,
) -> SoundSpec<Sound<Wire>> {
    SoundSpec {
        sound: Ref::Id(sound.into()),
        volume,
        pitch,
    }
}

fn to_wire_sound(sound: &Sound<Resolved>) -> Sound<Wire> {
    match sound {
        Ref::Id(id) => Ref::Id((*id).into()),
        Ref::Inline(value) => Ref::Inline(Arc::clone(value)),
    }
}

fn resolve_damage_id(
    registries: &CurrentRegistries,
    key: ResourceKey<'static, DamageType>,
) -> Option<u32> {
    registries
        .id_of(DAMAGE_TYPE.location(), key.location())
        .map(u32::from)
}

fn resolve_sound_id(registries: &CurrentRegistries, key: SoundEvent) -> Option<Id<SoundEvent>> {
    registries.entries::<SoundEvent>()?.by_name(key.location())
}

fn resolve_block_fall_sound(
    definitions: &BlockDefinitions,
    state_id: BlockStateId,
    sound_sets: Option<&BlockSoundSets>,
    sound_registry: Option<&Registry<BlockSoundSet>>,
) -> Option<FallSound> {
    let sound_set_index = definitions.state(state_id).sound_set?;
    let registry = sound_registry?;
    let set_id = if let Some(sets) = sound_sets {
        sets.get(sound_set_index)?
    } else {
        let key = definitions.sound_set(sound_set_index)?;
        registry.by_name(key.location())?
    };
    let set = registry.get(set_id)?;
    let fall_sound = set.fall_sound.as_ref()?;
    Some(FallSound {
        sound: to_wire_sound(fall_sound),
        volume: set.volume * 0.5_f32,
        pitch: set.pitch * 0.75_f32,
    })
}

fn is_dripstone_tip(entry: &BlockEntry, id: BlockStateId) -> bool {
    entry
        .value_of(id, "vertical_direction")
        .is_some_and(|value| value.renders_to("up"))
        && entry
            .value_of(id, "thickness")
            .is_some_and(|value| value.renders_to("tip"))
}

fn int_property(entry: &BlockEntry, id: BlockStateId, property: &str) -> u8 {
    let Some(value) = entry.value_of(id, property) else {
        return 0;
    };
    if value.renders_to("0") {
        0
    } else if value.renders_to("1") {
        1
    } else if value.renders_to("2") {
        2
    } else {
        3
    }
}

fn classify_fall_kind(entry: &BlockEntry, id: BlockStateId, is_bed: bool) -> FallBlockKind {
    let name = entry.identifier.as_str();
    if is_bed {
        FallBlockKind::Bed
    } else if name == Block::HayBlock.as_static_str() {
        FallBlockKind::HayBlock
    } else if name == Block::HoneyBlock.as_static_str() {
        FallBlockKind::HoneyBlock
    } else if name == Block::SlimeBlock.as_static_str() {
        FallBlockKind::SlimeBlock
    } else if name == Block::ShelfMushroom.as_static_str() {
        FallBlockKind::ShelfMushroom
    } else if name == Block::PowderSnow.as_static_str() {
        FallBlockKind::PowderSnow
    } else if (name == Block::PointedDripstone.as_static_str()
        || name == Block::Icicle.as_static_str())
        && is_dripstone_tip(entry, id)
    {
        FallBlockKind::PointedDripstoneTip
    } else {
        FallBlockKind::Default
    }
}

fn classify_contact_block(entry: &BlockEntry, id: BlockStateId) -> EnvContactBlock {
    let name = entry.identifier.as_str();
    let lit = entry
        .value_of(id, "lit")
        .is_some_and(|value| value.renders_to("true"));
    if name == Block::Cactus.as_static_str() {
        EnvContactBlock::Cactus
    } else if name == Block::Campfire.as_static_str() && lit {
        EnvContactBlock::CampfireLit
    } else if name == Block::SoulCampfire.as_static_str() && lit {
        EnvContactBlock::SoulCampfireLit
    } else if name == Block::SweetBerryBush.as_static_str() {
        EnvContactBlock::SweetBerryBush(int_property(entry, id, "age"))
    } else if name == Block::MagmaBlock.as_static_str() {
        EnvContactBlock::MagmaBlock
    } else if name == Block::Fire.as_static_str() {
        EnvContactBlock::Fire
    } else if name == Block::SoulFire.as_static_str() {
        EnvContactBlock::SoulFire
    } else if name == Block::LavaCauldron.as_static_str() {
        EnvContactBlock::LavaCauldron
    } else if name == Block::WaterCauldron.as_static_str() {
        EnvContactBlock::WaterCauldron(int_property(entry, id, "level"))
    } else if name == Block::PowderSnowCauldron.as_static_str() {
        EnvContactBlock::PowderSnowCauldron(int_property(entry, id, "level"))
    } else if name == Block::PowderSnow.as_static_str() {
        EnvContactBlock::PowderSnow
    } else if name == Block::BubbleColumn.as_static_str() {
        EnvContactBlock::BubbleColumn
    } else {
        EnvContactBlock::None
    }
}

fn classify_fluid(blocks: &Blocks, state_id: BlockStateId) -> EnvFluidKind {
    match blocks.state(state_id).fluid {
        Some(fluid_state) if fluid_state.fluid == Fluid::Water.id() => {
            EnvFluidKind::Water(fluid_state.level)
        }
        Some(fluid_state) if fluid_state.fluid == Fluid::Lava.id() => {
            EnvFluidKind::Lava(fluid_state.level)
        }
        _ => EnvFluidKind::None,
    }
}

fn bake_registry_tables(registries: &CurrentRegistries) -> EnvRegistryTables {
    let rain_biomes: Box<[bool]> =
        registries
            .registry::<BiomeFile>()
            .map_or_else(Box::default, |biomes| {
                let entries: Vec<bool> = biomes
                    .iter()
                    .map(|(_, _, biome)| {
                        biome.has_precipitation && weather::warm_enough_for_rain(biome.temperature)
                    })
                    .collect();
                entries.into_boxed_slice()
            });

    let item_count = registries
        .entries::<Item>()
        .map_or(0, |entries| entries.len());
    let mut freeze_immune_items = vec![false; item_count];
    if let Some(item_tags) = registries.tags::<Item>()
        && let Some(tag_id) = item_tags.id(item_tag_keys::FREEZE_IMMUNE_WEARABLES)
        && let Some(item_registry) = registries.entries::<Item>()
    {
        for (item_id, _) in item_registry.iter() {
            if item_tags.contains(tag_id, item_id)
                && let Some(slot) = freeze_immune_items.get_mut(item_id.index())
            {
                *slot = true;
            }
        }
    }

    EnvRegistryTables {
        rain_biomes,
        freeze_immune_items: freeze_immune_items.into_boxed_slice(),
        item_count,
    }
}

fn bake_damage_and_sound_ids(registries: &CurrentRegistries) -> BakedEnvDamage {
    BakedEnvDamage {
        fall: resolve_damage_id(registries, damage_type_keys::FALL),
        stalagmite: resolve_damage_id(registries, damage_type_keys::STALAGMITE),
        in_wall: resolve_damage_id(registries, damage_type_keys::IN_WALL),
        cactus: resolve_damage_id(registries, damage_type_keys::CACTUS),
        campfire: resolve_damage_id(registries, damage_type_keys::CAMPFIRE),
        sweet_berry_bush: resolve_damage_id(registries, damage_type_keys::SWEET_BERRY_BUSH),
        hot_floor: resolve_damage_id(registries, damage_type_keys::HOT_FLOOR),
        out_of_world: resolve_damage_id(registries, damage_type_keys::OUT_OF_WORLD),
        outside_border: resolve_damage_id(registries, damage_type_keys::OUTSIDE_BORDER),
        on_fire: resolve_damage_id(registries, damage_type_keys::ON_FIRE),
        in_fire: resolve_damage_id(registries, damage_type_keys::IN_FIRE),
        lava: resolve_damage_id(registries, damage_type_keys::LAVA),
        drown: resolve_damage_id(registries, damage_type_keys::DROWN),
        freeze: resolve_damage_id(registries, damage_type_keys::FREEZE),
        player_small_fall: resolve_sound_id(registries, SoundEvent::EntityPlayerSmallFall),
        player_big_fall: resolve_sound_id(registries, SoundEvent::EntityPlayerBigFall),
        generic_small_fall: resolve_sound_id(registries, SoundEvent::EntityGenericSmallFall),
        generic_big_fall: resolve_sound_id(registries, SoundEvent::EntityGenericBigFall),
        honey_slide: resolve_sound_id(registries, SoundEvent::BlockHoneyBlockSlide),
        generic_burn: resolve_sound_id(registries, SoundEvent::EntityGenericBurn),
        extinguish_fire: resolve_sound_id(registries, SoundEvent::EntityGenericExtinguishFire),
    }
}

pub(crate) fn bake_env_damage(
    registries: Res<CurrentRegistries>,
    blocks: Res<Blocks>,
    sound_sets: Option<Res<BlockSoundSets>>,
    mut baked: ResMut<BakedEnvDamage>,
    mut table: ResMut<EnvBlockTable>,
    mut registry_tables: ResMut<EnvRegistryTables>,
) {
    baked.set_if_neq(bake_damage_and_sound_ids(&registries));
    registry_tables.set_if_neq(bake_registry_tables(&registries));

    let mut entries = vec![EnvBlockEntry::default(); blocks.state_count()];
    let tags = registries.tags::<Block>();
    let bed_tag = tags
        .as_ref()
        .and_then(|registry_tags| registry_tags.id(block_tag_keys::BEDS));
    let motion_tag = tags
        .as_ref()
        .and_then(|registry_tags| registry_tags.id(block_tag_keys::BLOCKS_MOTION));
    let sound_registry = registries.registry::<BlockSoundSet>();

    for block_entry in blocks.blocks() {
        let block_id = blocks.block_index(block_entry.base_state_id);
        let is_bed = bed_tag
            .zip(tags.as_ref())
            .zip(block_id)
            .is_some_and(|((tag_id, registry_tags), id)| registry_tags.contains(tag_id, id));
        let blocks_motion = motion_tag
            .zip(tags.as_ref())
            .zip(block_id)
            .is_some_and(|((tag_id, registry_tags), id)| registry_tags.contains(tag_id, id));
        for offset in 0..block_entry.state_count {
            let Some(state_id) = block_entry.state_at(offset) else {
                continue;
            };
            let fall_kind = classify_fall_kind(block_entry, state_id, is_bed);
            let contact = classify_contact_block(block_entry, state_id);
            let fluid = classify_fluid(&blocks, state_id);
            let fall_sound =
                resolve_block_fall_sound(&blocks, state_id, sound_sets.as_deref(), sound_registry);
            if let Some(slot) = entries.get_mut(state_id.index()) {
                *slot = EnvBlockEntry {
                    fall_kind,
                    contact,
                    fluid,
                    blocks_motion,
                    fall_sound,
                };
            }
        }
    }

    table.set_if_neq(EnvBlockTable(entries.into_boxed_slice()));
}
