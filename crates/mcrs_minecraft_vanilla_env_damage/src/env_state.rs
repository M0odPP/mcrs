use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::Message;
use bevy_ecs::schedule::SystemSet;
use glam::DVec3;
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
}

const _: () = {
    assert!(
        size_of::<EnvTickState>() == 16,
        "EnvTickState size must match two f64 move deltas"
    );
    assert!(
        align_of::<EnvTickState>() == 8,
        "EnvTickState align must match f64 alignment"
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
