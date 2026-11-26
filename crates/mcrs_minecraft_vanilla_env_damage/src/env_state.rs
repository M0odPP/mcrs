use bevy_ecs::component::Component;
use bevy_ecs::schedule::SystemSet;

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
