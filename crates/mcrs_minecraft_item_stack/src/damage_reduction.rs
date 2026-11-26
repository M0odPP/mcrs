use core::hash::Hash;
use core::hash::Hasher;
use core::mem::align_of;
use core::mem::size_of;

use crate::DamageTypeSet;
use crate::float_key;

#[derive(Clone, Debug)]
pub struct DamageReduction {
    pub horizontal_blocking_angle: f32,
    pub kind: Option<DamageTypeSet>,
    pub base: f32,
    pub factor: f32,
}

const _: () = {
    assert!(
        size_of::<DamageReduction>() >= size_of::<Option<DamageTypeSet>>() + 12,
        "DamageReduction packs optional DamageTypeSet and three floats"
    );
    assert!(
        align_of::<DamageReduction>() == align_of::<DamageTypeSet>(),
        "DamageReduction aligns with DamageTypeSet"
    );
};

impl DamageReduction {
    pub const DEFAULT_HORIZONTAL_BLOCKING_ANGLE: f32 = 90.0;
    pub const DEFAULT_BASE: f32 = 0.0;
    pub const DEFAULT_FACTOR: f32 = 1.0;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            horizontal_blocking_angle: Self::DEFAULT_HORIZONTAL_BLOCKING_ANGLE,
            kind: None,
            base: Self::DEFAULT_BASE,
            factor: Self::DEFAULT_FACTOR,
        }
    }

    pub(crate) fn is_default_exact(&self) -> bool {
        self.horizontal_blocking_angle.to_bits()
            == Self::DEFAULT_HORIZONTAL_BLOCKING_ANGLE.to_bits()
            && self.kind.is_none()
            && self.base.to_bits() == Self::DEFAULT_BASE.to_bits()
            && self.factor.to_bits() == Self::DEFAULT_FACTOR.to_bits()
    }
}

impl Default for DamageReduction {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for DamageReduction {
    fn eq(&self, other: &Self) -> bool {
        float_key::of_f32(self.horizontal_blocking_angle)
            == float_key::of_f32(other.horizontal_blocking_angle)
            && self.kind == other.kind
            && float_key::of_f32(self.base) == float_key::of_f32(other.base)
            && float_key::of_f32(self.factor) == float_key::of_f32(other.factor)
    }
}

impl Eq for DamageReduction {}

impl Hash for DamageReduction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        float_key::of_f32(self.horizontal_blocking_angle).hash(state);
        self.kind.hash(state);
        float_key::of_f32(self.base).hash(state);
        float_key::of_f32(self.factor).hash(state);
    }
}
