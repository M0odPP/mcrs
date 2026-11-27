use crate::ItemStack;

#[derive(Clone, Debug, PartialEq, Hash)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct UseRemainder(pub ItemStack);
