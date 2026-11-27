#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct AdditionalTradeCost(pub i32);
