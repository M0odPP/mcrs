use core::hash::{Hash, Hasher};
use core::mem::{align_of, size_of};

use crate::float_key;

#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct MinimumAttackCharge(pub f32);

const _: () = assert!(
    size_of::<MinimumAttackCharge>() == 4 && align_of::<MinimumAttackCharge>() == 4,
    "MinimumAttackCharge is 4 bytes with 4-byte alignment"
);

impl MinimumAttackCharge {
    #[must_use]
    pub fn new(value: f32) -> Option<Self> {
        (value.total_cmp(&0.0).is_ge() && value.total_cmp(&1.0).is_le()).then_some(Self(value))
    }
}

impl PartialEq for MinimumAttackCharge {
    fn eq(&self, other: &Self) -> bool {
        float_key::of_f32(self.0) == float_key::of_f32(other.0)
    }
}
impl Eq for MinimumAttackCharge {}
impl Hash for MinimumAttackCharge {
    fn hash<H: Hasher>(&self, state: &mut H) {
        float_key::of_f32(self.0).hash(state);
    }
}
