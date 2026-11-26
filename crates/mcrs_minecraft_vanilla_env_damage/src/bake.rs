use std::sync::Arc;

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::prelude::{Res, ResMut, Resource};
use mcrs_minecraft_block::block_keys::Block;
use mcrs_minecraft_block::block_tag_keys;
use mcrs_minecraft_block::definition::{BlockDefinitions, BlockEntry, BlockSoundSets, Blocks};
use mcrs_minecraft_entity::damage_type::DamageType;
use mcrs_minecraft_entity::damage_type_keys;
use mcrs_minecraft_entity::registry_keys::DAMAGE_TYPE;
use mcrs_minecraft_env_damage::FallBlockKind;
use mcrs_minecraft_registries::{CurrentRegistries, Ref, Registry, Resolved, Wire};
use mcrs_minecraft_resource::{Id, ResourceKey};
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use mcrs_minecraft_sound::{BlockSoundSet, Sound};
use mcrs_voxel::BlockStateId;

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
    pub player_small_fall: Option<Id<SoundEvent>>,
    pub player_big_fall: Option<Id<SoundEvent>>,
    pub generic_small_fall: Option<Id<SoundEvent>>,
    pub generic_big_fall: Option<Id<SoundEvent>>,
    pub honey_slide: Option<Id<SoundEvent>>,
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
    pub fall_sound: Option<FallSound>,
}

impl Default for EnvBlockEntry {
    fn default() -> Self {
        Self {
            fall_kind: FallBlockKind::Default,
            contact: EnvContactBlock::None,
            fall_sound: None,
        }
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct EnvBlockTable(Box<[EnvBlockEntry]>);

const _: () = {
    assert!(
        size_of::<BakedEnvDamage>() == 92,
        "BakedEnvDamage size must match cached ids"
    );
    assert!(
        align_of::<BakedEnvDamage>() == 4,
        "BakedEnvDamage align must match u32 alignment"
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
        size_of::<EnvBlockEntry>() == 24 || size_of::<EnvBlockEntry>() == 32,
        "EnvBlockEntry size must stay compact"
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

fn sweet_berry_age(entry: &BlockEntry, id: BlockStateId) -> u8 {
    let Some(value) = entry.value_of(id, "age") else {
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

fn classify_block_state(
    entry: &BlockEntry,
    id: BlockStateId,
    is_bed: bool,
) -> (FallBlockKind, EnvContactBlock) {
    let name = entry.identifier.as_str();
    let fall_kind = if is_bed {
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
    };

    let lit = entry
        .value_of(id, "lit")
        .is_some_and(|value| value.renders_to("true"));
    let contact = if name == Block::Cactus.as_static_str() {
        EnvContactBlock::Cactus
    } else if name == Block::Campfire.as_static_str() && lit {
        EnvContactBlock::CampfireLit
    } else if name == Block::SoulCampfire.as_static_str() && lit {
        EnvContactBlock::SoulCampfireLit
    } else if name == Block::SweetBerryBush.as_static_str() {
        EnvContactBlock::SweetBerryBush(sweet_berry_age(entry, id))
    } else if name == Block::MagmaBlock.as_static_str() {
        EnvContactBlock::MagmaBlock
    } else {
        EnvContactBlock::None
    };

    (fall_kind, contact)
}

pub(crate) fn bake_env_damage(
    registries: Res<CurrentRegistries>,
    blocks: Res<Blocks>,
    sound_sets: Option<Res<BlockSoundSets>>,
    mut baked: ResMut<BakedEnvDamage>,
    mut table: ResMut<EnvBlockTable>,
) {
    let next_baked = BakedEnvDamage {
        fall: resolve_damage_id(&registries, damage_type_keys::FALL),
        stalagmite: resolve_damage_id(&registries, damage_type_keys::STALAGMITE),
        in_wall: resolve_damage_id(&registries, damage_type_keys::IN_WALL),
        cactus: resolve_damage_id(&registries, damage_type_keys::CACTUS),
        campfire: resolve_damage_id(&registries, damage_type_keys::CAMPFIRE),
        sweet_berry_bush: resolve_damage_id(&registries, damage_type_keys::SWEET_BERRY_BUSH),
        hot_floor: resolve_damage_id(&registries, damage_type_keys::HOT_FLOOR),
        out_of_world: resolve_damage_id(&registries, damage_type_keys::OUT_OF_WORLD),
        outside_border: resolve_damage_id(&registries, damage_type_keys::OUTSIDE_BORDER),
        player_small_fall: resolve_sound_id(&registries, SoundEvent::EntityPlayerSmallFall),
        player_big_fall: resolve_sound_id(&registries, SoundEvent::EntityPlayerBigFall),
        generic_small_fall: resolve_sound_id(&registries, SoundEvent::EntityGenericSmallFall),
        generic_big_fall: resolve_sound_id(&registries, SoundEvent::EntityGenericBigFall),
        honey_slide: resolve_sound_id(&registries, SoundEvent::BlockHoneyBlockSlide),
    };
    baked.set_if_neq(next_baked);

    let mut entries = vec![EnvBlockEntry::default(); blocks.state_count()];
    let tags = registries.tags::<Block>();
    let bed_tag = tags
        .as_ref()
        .and_then(|registry_tags| registry_tags.id(block_tag_keys::BEDS));
    let sound_registry = registries.registry::<BlockSoundSet>();

    for block_entry in blocks.blocks() {
        let is_bed = bed_tag
            .zip(tags.as_ref())
            .is_some_and(|(tag_id, registry_tags)| {
                blocks
                    .block_index(block_entry.base_state_id)
                    .is_some_and(|block_id| registry_tags.contains(tag_id, block_id))
            });
        for offset in 0..block_entry.state_count {
            let Some(state_id) = block_entry.state_at(offset) else {
                continue;
            };
            let (fall_kind, contact) = classify_block_state(block_entry, state_id, is_bed);
            let fall_sound =
                resolve_block_fall_sound(&blocks, state_id, sound_sets.as_deref(), sound_registry);
            if let Some(slot) = entries.get_mut(state_id.index()) {
                *slot = EnvBlockEntry {
                    fall_kind,
                    contact,
                    fall_sound,
                };
            }
        }
    }

    table.set_if_neq(EnvBlockTable(entries.into_boxed_slice()));
}
