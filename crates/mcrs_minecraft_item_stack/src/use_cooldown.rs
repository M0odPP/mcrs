use core::hash::{Hash, Hasher};
use core::mem::{align_of, size_of};

use mcrs_minecraft_resource::{ResourceLocation, ResourceLocationBuf};

use crate::float_key;
use crate::identifier::Identifier;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct UseCooldown {
    pub seconds: f32,
    cooldown_group: Option<Identifier>,
}

const _: () = assert!(
    size_of::<UseCooldown>() >= size_of::<Option<Identifier>>() + size_of::<f32>()
        && align_of::<UseCooldown>() == align_of::<Option<Identifier>>(),
    "UseCooldown stores seconds and optional cooldown_group"
);

impl UseCooldown {
    #[must_use]
    pub const fn new(seconds: f32) -> Self {
        Self {
            seconds,
            cooldown_group: None,
        }
    }

    #[must_use]
    pub fn with_cooldown_group(mut self, group: ResourceLocationBuf) -> Self {
        self.cooldown_group = Some(Identifier::Owned(group));
        self
    }

    #[must_use]
    pub fn with_static_cooldown_group(mut self, group: ResourceLocation<'static>) -> Self {
        self.cooldown_group = Some(Identifier::Borrowed(group));
        self
    }

    #[must_use]
    pub fn cooldown_group(&self) -> Option<ResourceLocation<'_>> {
        self.cooldown_group.as_ref().map(Identifier::as_location)
    }

    pub(crate) fn from_parts(seconds: f32, cooldown_group: Option<ResourceLocationBuf>) -> Self {
        Self {
            seconds,
            cooldown_group: cooldown_group.map(Identifier::Owned),
        }
    }

    pub(crate) fn borrow_static(&'static self) -> Self {
        Self {
            seconds: self.seconds,
            cooldown_group: self.cooldown_group.as_ref().map(Identifier::borrow_static),
        }
    }
}

impl PartialEq for UseCooldown {
    fn eq(&self, other: &Self) -> bool {
        float_key::of_f32(self.seconds) == float_key::of_f32(other.seconds)
            && self.cooldown_group == other.cooldown_group
    }
}

impl Eq for UseCooldown {}

impl Hash for UseCooldown {
    fn hash<H: Hasher>(&self, state: &mut H) {
        float_key::of_f32(self.seconds).hash(state);
        self.cooldown_group.hash(state);
    }
}
