use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::prelude::{Query, Res};
use bevy_ecs::system::SystemParam;
use glam::DVec3;
use mcrs_minecraft_dimension_environment::environment::WeatherLevel;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine_world::palette::BiomePalette;
use mcrs_minecraft_engine_world::world::storage::section::SectionIndex;
use mcrs_minecraft_engine_world::world::storage::view::DimensionView;
use mcrs_minecraft_env_damage::{
    self as env_damage, FireImmunity, FireSource, LAVA_DAMAGE, LAVA_IGNITE_TICKS,
    PLAYER_FIRE_GRACE, WetExposure,
};
use mcrs_minecraft_physics::{self as physics, Aabb};
use mcrs_minecraft_pose::Dimensions;
use mcrs_voxel::{BlockPos, LocalPos, SectionPos};

use crate::bake::{
    self, BakedEnvDamage, EnvBlockEntry, EnvBlockTable, EnvContactBlock, EnvFluidKind,
};
use crate::env_state::{self, PendingEnvEffect};
use crate::registry_tables::EnvRegistryTables;
use crate::{fall_system, water_hazards};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FireContact {
    #[default]
    None,
    Fire,
    SoulFire,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LavaContact {
    #[default]
    None,
    Lava,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ExtinguishStatus {
    #[default]
    Unchanged,
    Extinguished,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SurroundingsScan {
    pub(crate) fire_contact: FireContact,
    pub(crate) lava_contact: LavaContact,
    pub(crate) wet: WetExposure,
}

#[derive(SystemParam)]
pub(crate) struct RainWorld<'w, 's> {
    pub(crate) weather_level: Option<Res<'w, WeatherLevel>>,
    pub(crate) section_biomes: Query<'w, 's, &'static BiomePalette>,
}

pub(crate) struct RainContext<'a> {
    pub(crate) has_rain: bool,
    pub(crate) min_y: i32,
    pub(crate) max_y: i32,
    pub(crate) dimension_view: Option<&'a DimensionView<'a>>,
    pub(crate) block_table: &'a EnvBlockTable,
    pub(crate) registry_tables: &'a EnvRegistryTables,
    pub(crate) section_index: Option<&'a SectionIndex>,
}

pub(crate) struct StatusInputs {
    pub(crate) player: Entity,
    pub(crate) position: DVec3,
    pub(crate) now: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlayerFireOutcome {
    pub(crate) next_fire_ticks: i16,
    pub(crate) extinguish_status: ExtinguishStatus,
    pub(crate) burning_damage: Option<f32>,
    pub(crate) in_fire_damage: Option<f32>,
    pub(crate) lava_damage: Option<f32>,
}

const _: () = {
    assert!(
        size_of::<FireContact>() == 1,
        "FireContact size must match single-byte enum"
    );
    assert!(
        align_of::<FireContact>() == 1,
        "FireContact alignment must match single-byte enum"
    );
    assert!(
        size_of::<LavaContact>() == 1,
        "LavaContact size must match single-byte enum"
    );
    assert!(
        align_of::<LavaContact>() == 1,
        "LavaContact alignment must match single-byte enum"
    );
    assert!(
        size_of::<ExtinguishStatus>() == 1,
        "ExtinguishStatus size must match single-byte enum"
    );
    assert!(
        align_of::<ExtinguishStatus>() == 1,
        "ExtinguishStatus alignment must match single-byte enum"
    );
    assert!(
        size_of::<SurroundingsScan>() == 3,
        "SurroundingsScan size must match fire, lava, and wetness enums"
    );
    assert!(
        align_of::<SurroundingsScan>() == 1,
        "SurroundingsScan alignment must match single-byte enums"
    );
    assert!(
        size_of::<RainContext<'static>>() == 48,
        "RainContext size must match world references and bounds"
    );
    assert!(
        align_of::<RainContext<'static>>() == 8,
        "RainContext alignment must match pointer alignment"
    );
    assert!(
        size_of::<StatusInputs>() == 40,
        "StatusInputs size must match player status fields"
    );
    assert!(
        align_of::<StatusInputs>() == 8,
        "StatusInputs alignment must match f64 alignment"
    );
    assert!(
        size_of::<PlayerFireOutcome>() == 28,
        "PlayerFireOutcome size must fit fire ticks, extinguish status, and three optional hits"
    );
    assert!(
        align_of::<PlayerFireOutcome>() == 4,
        "PlayerFireOutcome alignment must match f32 alignment"
    );
};

fn cauldron_fluid_box(block_position: BlockPos, top_sixteenths: f64) -> Aabb {
    let min_x = f64::from(block_position.x) + (2.0_f64 / 16.0_f64);
    let min_y = f64::from(block_position.y) + (4.0_f64 / 16.0_f64);
    let min_z = f64::from(block_position.z) + (2.0_f64 / 16.0_f64);
    let max_x = f64::from(block_position.x) + (14.0_f64 / 16.0_f64);
    let max_y = f64::from(block_position.y) + (top_sixteenths / 16.0_f64);
    let max_z = f64::from(block_position.z) + (14.0_f64 / 16.0_f64);
    Aabb::new(
        DVec3::new(min_x, min_y, min_z),
        DVec3::new(max_x, max_y, max_z),
    )
}

fn is_rain_blocking(entry: &EnvBlockEntry) -> bool {
    entry.blocks_motion || !matches!(entry.fluid, EnvFluidKind::None)
}

fn has_motion_blocking_above(
    rain: &RainContext<'_>,
    block_position: BlockPos,
    upper_y: i32,
) -> bool {
    let start_y = block_position.y.max(rain.min_y);
    if start_y > upper_y {
        return false;
    }
    let Some(section_index) = rain.section_index else {
        return false;
    };
    let min_section = start_y.div_euclid(16);
    let max_section = upper_y.div_euclid(16);
    let section_x = block_position.x.div_euclid(16);
    let section_z = block_position.z.div_euclid(16);
    for section_y in min_section..=max_section {
        let section_position = SectionPos::new(section_x, section_y, section_z);
        let Some(blocks) = section_index.blocks(section_position) else {
            continue;
        };
        let mut any_blocking = false;
        blocks.0.for_each_distinct(|state| {
            any_blocking |= rain.block_table.get(state).is_some_and(is_rain_blocking);
        });
        if !any_blocking {
            continue;
        }
        let section_min_y = (section_y * 16).max(start_y);
        let section_max_y = (section_y * 16 + 15).min(upper_y);
        for check_y in section_min_y..=section_max_y {
            let local_position =
                LocalPos::from(BlockPos::new(block_position.x, check_y, block_position.z));
            let state = blocks.0.get(local_position);
            if rain.block_table.get(state).is_some_and(is_rain_blocking) {
                return true;
            }
        }
    }
    false
}

fn is_raining_at(
    rain: &RainContext<'_>,
    rain_world: &RainWorld<'_, '_>,
    block_position: BlockPos,
) -> bool {
    let Some(section_index) = rain.section_index else {
        return false;
    };
    let clamped_y = block_position.y.clamp(rain.min_y, rain.max_y);
    let biome_position = BlockPos::new(block_position.x, clamped_y, block_position.z);
    let allows_rain = section_index
        .get(SectionPos::from(biome_position))
        .and_then(|entity| rain_world.section_biomes.get(entity).ok())
        .is_some_and(|biomes| {
            rain.registry_tables
                .is_rain_biome(usize::from(biomes.get(LocalPos::from(biome_position))))
        });
    allows_rain && !has_motion_blocking_above(rain, block_position, rain.max_y)
}

fn is_player_in_rain(
    rain: &RainContext<'_>,
    rain_world: &RainWorld<'_, '_>,
    position: DVec3,
    height: f32,
) -> bool {
    if !rain.has_rain {
        return false;
    }
    let feet = BlockPos::new(
        physics::floor_block(position.x),
        physics::floor_block(position.y),
        physics::floor_block(position.z),
    );
    let head_position = BlockPos::new(
        feet.x,
        physics::floor_block(position.y + f64::from(height)),
        feet.z,
    );
    is_raining_at(rain, rain_world, feet)
        || (head_position != feet && is_raining_at(rain, rain_world, head_position))
}

fn accumulate_block_cell(
    rain: &RainContext<'_>,
    shrunk: Aabb,
    block_position: BlockPos,
    accumulation: &mut SurroundingsScan,
) {
    let state = fall_system::block_state_at(rain.dimension_view, block_position);
    let Some(entry) = rain.block_table.get(state) else {
        return;
    };
    match entry.fluid {
        EnvFluidKind::Water(_) => {
            let fluid_top = water_hazards::fluid_top_y(
                rain.dimension_view,
                rain.block_table,
                block_position,
                entry.fluid,
            );
            if shrunk.min.y < fluid_top {
                accumulation.wet = WetExposure::ExtinguishedByWetness;
            }
        }
        EnvFluidKind::Lava(_) => {
            let fluid_top = water_hazards::fluid_top_y(
                rain.dimension_view,
                rain.block_table,
                block_position,
                entry.fluid,
            );
            if shrunk.min.y < fluid_top {
                accumulation.lava_contact = LavaContact::Lava;
            }
        }
        EnvFluidKind::None => {}
    }
    match entry.contact {
        EnvContactBlock::Fire if matches!(accumulation.fire_contact, FireContact::None) => {
            accumulation.fire_contact = FireContact::Fire;
        }
        EnvContactBlock::SoulFire => {
            accumulation.fire_contact = FireContact::SoulFire;
        }
        EnvContactBlock::LavaCauldron
            if shrunk.intersects(&cauldron_fluid_box(block_position, 15.0_f64)) =>
        {
            accumulation.lava_contact = LavaContact::Lava;
        }
        EnvContactBlock::WaterCauldron(level) | EnvContactBlock::PowderSnowCauldron(level)
            if level > 0
                && shrunk.intersects(&cauldron_fluid_box(
                    block_position,
                    6.0_f64 + 3.0_f64 * f64::from(level),
                )) =>
        {
            accumulation.wet = WetExposure::ExtinguishedByWetness;
        }
        EnvContactBlock::PowderSnow => {
            accumulation.wet = WetExposure::ExtinguishedByWetness;
        }
        _ => {}
    }
}

#[must_use]
pub(crate) fn scan_surroundings(
    rain: &RainContext<'_>,
    rain_world: &RainWorld<'_, '_>,
    position: DVec3,
    body: Dimensions,
    shrunk: Aabb,
) -> SurroundingsScan {
    let mut accumulation = SurroundingsScan {
        fire_contact: FireContact::None,
        lava_contact: LavaContact::None,
        wet: if is_player_in_rain(rain, rain_world, position, body.height) {
            WetExposure::ExtinguishedByWetness
        } else {
            WetExposure::Dry
        },
    };

    let (min_x, min_y, min_z) = (
        physics::floor_block(shrunk.min.x),
        physics::floor_block(shrunk.min.y),
        physics::floor_block(shrunk.min.z),
    );
    let (max_x, max_y, max_z) = (
        physics::floor_block(shrunk.max.x),
        physics::floor_block(shrunk.max.y),
        physics::floor_block(shrunk.max.z),
    );

    for block_y in min_y..=max_y {
        for block_z in min_z..=max_z {
            for block_x in min_x..=max_x {
                let inside_cell = shrunk.min.x < f64::from(block_x + 1)
                    && shrunk.max.x > f64::from(block_x)
                    && shrunk.min.y < f64::from(block_y + 1)
                    && shrunk.max.y > f64::from(block_y)
                    && shrunk.min.z < f64::from(block_z + 1)
                    && shrunk.max.z > f64::from(block_z);
                if inside_cell {
                    let block_position = BlockPos::new(block_x, block_y, block_z);
                    accumulate_block_cell(rain, shrunk, block_position, &mut accumulation);
                }
            }
        }
    }

    accumulation
}

#[must_use]
pub(crate) fn step_player_fire(
    file_state: &PlayerFileState,
    surroundings: &SurroundingsScan,
) -> PlayerFireOutcome {
    let fire_source = match surroundings.fire_contact {
        FireContact::None => FireSource::None,
        FireContact::Fire => FireSource::InFire,
        FireContact::SoulFire => FireSource::InSoulFire,
    };
    match (surroundings.fire_contact, surroundings.lava_contact) {
        (FireContact::None, LavaContact::None) => {
            let outcome = env_damage::step_fire(
                file_state.fire,
                PLAYER_FIRE_GRACE,
                FireSource::None,
                surroundings.wet,
                FireImmunity::Vulnerable,
            );
            PlayerFireOutcome {
                next_fire_ticks: outcome.next_fire_ticks,
                extinguish_status: if outcome.extinguished {
                    ExtinguishStatus::Extinguished
                } else {
                    ExtinguishStatus::Unchanged
                },
                burning_damage: outcome.burning_damage,
                in_fire_damage: None,
                lava_damage: None,
            }
        }
        (FireContact::Fire | FireContact::SoulFire, LavaContact::None) => {
            let outcome = env_damage::step_fire(
                file_state.fire,
                PLAYER_FIRE_GRACE,
                fire_source,
                surroundings.wet,
                FireImmunity::Vulnerable,
            );
            PlayerFireOutcome {
                next_fire_ticks: outcome.next_fire_ticks,
                extinguish_status: if outcome.extinguished {
                    ExtinguishStatus::Extinguished
                } else {
                    ExtinguishStatus::Unchanged
                },
                burning_damage: outcome.burning_damage,
                in_fire_damage: outcome.contact_damage,
                lava_damage: None,
            }
        }
        (FireContact::None, LavaContact::Lava) => {
            let outcome = env_damage::step_fire(
                file_state.fire,
                PLAYER_FIRE_GRACE,
                FireSource::InLava,
                surroundings.wet,
                FireImmunity::Vulnerable,
            );
            PlayerFireOutcome {
                next_fire_ticks: outcome.next_fire_ticks,
                extinguish_status: if outcome.extinguished {
                    ExtinguishStatus::Extinguished
                } else {
                    ExtinguishStatus::Unchanged
                },
                burning_damage: outcome.burning_damage,
                in_fire_damage: None,
                lava_damage: outcome.contact_damage,
            }
        }
        (FireContact::Fire | FireContact::SoulFire, LavaContact::Lava) => {
            let after_fire = env_damage::step_fire(
                file_state.fire,
                PLAYER_FIRE_GRACE,
                fire_source,
                WetExposure::Dry,
                FireImmunity::Vulnerable,
            );
            let ignited_ticks = env_damage::ignite(after_fire.next_fire_ticks, LAVA_IGNITE_TICKS);
            let (next_fire_ticks, extinguish_status) = match surroundings.wet {
                WetExposure::ExtinguishedByWetness => (
                    PLAYER_FIRE_GRACE,
                    if ignited_ticks > 0 {
                        ExtinguishStatus::Extinguished
                    } else {
                        ExtinguishStatus::Unchanged
                    },
                ),
                WetExposure::Dry => (ignited_ticks, ExtinguishStatus::Unchanged),
            };
            PlayerFireOutcome {
                next_fire_ticks,
                extinguish_status,
                burning_damage: None,
                in_fire_damage: after_fire.contact_damage,
                lava_damage: Some(LAVA_DAMAGE),
            }
        }
    }
}

pub(crate) fn step_extinguish_sound(
    inputs: &StatusInputs,
    baked: &BakedEnvDamage,
    file_state: &PlayerFileState,
    current_cooldown: u8,
    extinguish_status: ExtinguishStatus,
    effects: &mut MessageWriter<PendingEnvEffect>,
) -> u8 {
    let mut next_cooldown = current_cooldown.saturating_sub(1);
    if extinguish_status == ExtinguishStatus::Extinguished && next_cooldown == 0 {
        next_cooldown = 20;
        if !file_state.silent
            && let Some(extinguish_sound) = baked.extinguish_fire
        {
            let jitter = (env_state::unit_random(inputs.now, inputs.player, 11)
                - env_state::unit_random(inputs.now, inputs.player, 12))
                * 0.4_f32;
            let pitch = 1.6_f32 + jitter;
            effects.write(PendingEnvEffect {
                actor: inputs.player,
                position: inputs.position,
                entity_sound: Some(bake::sound_spec_wire(extinguish_sound, 0.7_f32, pitch)),
                block_sound: None,
                honey_slide: false,
            });
        }
    }
    next_cooldown
}
