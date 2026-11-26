use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Message;
use bevy_ecs::schedule::SystemSet;
use glam::DVec3;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_entity::mob_effect_keys::MobEffect;
use mcrs_minecraft_registries::Wire;
use mcrs_minecraft_sound::Sound;

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvDamageSet {
    Collect,
    Project,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct EnvTickState {
    pub tick_move_x: f64,
    pub tick_move_z: f64,
    pub freeze_age_ticks: u64,
    pub extinguish_sound_cooldown: u8,
}

const _: () = {
    assert!(
        size_of::<EnvTickState>() == 32,
        "EnvTickState size must match movement deltas, freeze age, and cooldown"
    );
    assert!(
        align_of::<EnvTickState>() == 8,
        "EnvTickState alignment must match f64 alignment"
    );
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SoundSpec<S> {
    pub(crate) sound: S,
    pub(crate) volume: f32,
    pub(crate) pitch: f32,
}

#[derive(Message, Clone, Debug)]
pub(crate) struct PendingEnvEffect {
    pub(crate) actor: Entity,
    pub(crate) position: DVec3,
    pub(crate) entity_sound: Option<SoundSpec<Sound<Wire>>>,
    pub(crate) block_sound: Option<SoundSpec<Sound<Wire>>>,
    pub(crate) honey_slide: bool,
}

#[must_use]
pub(crate) fn has_active_effect(file_state: &PlayerFileState, expected: MobEffect) -> bool {
    let full = expected.as_static_str();
    let short = full.strip_prefix("minecraft:").unwrap_or(full);
    file_state.active_effects.iter().any(|effect| {
        (effect.duration == -1 || effect.duration > 0) && (effect.id == full || effect.id == short)
    })
}

fn mix_u64(value: u64) -> u64 {
    let mixed = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    mixed ^ (mixed >> 31)
}

fn unit_f32(mixed: u64) -> f32 {
    let bits = (mixed >> 40) as u32;
    (bits as f32) * (1.0_f32 / ((1_u32 << 24) as f32))
}

#[must_use]
pub(crate) fn unit_random(tick: u64, actor: Entity, salt: u64) -> f32 {
    let seed = tick.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ actor.to_bits()
        ^ salt.wrapping_mul(0x517c_c1b7_2722_0a95);
    unit_f32(mix_u64(seed))
}
