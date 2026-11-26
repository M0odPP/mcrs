use core::hash::Hash;
use core::hash::Hasher;
use core::mem::align_of;
use core::mem::size_of;

use crate::float_key;

#[derive(Clone, Copy, Debug)]
pub struct ItemDamage {
    pub threshold: f32,
    pub base: f32,
    pub factor: f32,
}

const _: () = {
    assert!(
        size_of::<ItemDamage>() == 12,
        "ItemDamage packs three f32 fields into 12 bytes"
    );
    assert!(
        align_of::<ItemDamage>() == 4,
        "ItemDamage aligns to 4 bytes"
    );
};

impl ItemDamage {
    pub const DEFAULT_THRESHOLD: f32 = 1.0;
    pub const DEFAULT_BASE: f32 = 0.0;
    pub const DEFAULT_FACTOR: f32 = 1.0;
    pub const DEFAULT: Self = Self {
        threshold: Self::DEFAULT_THRESHOLD,
        base: Self::DEFAULT_BASE,
        factor: Self::DEFAULT_FACTOR,
    };

    pub(crate) fn is_default_exact(self) -> bool {
        self.threshold.to_bits() == Self::DEFAULT_THRESHOLD.to_bits()
            && self.base.to_bits() == Self::DEFAULT_BASE.to_bits()
            && self.factor.to_bits() == Self::DEFAULT_FACTOR.to_bits()
    }
}

impl Default for ItemDamage {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl PartialEq for ItemDamage {
    fn eq(&self, other: &Self) -> bool {
        float_key::of_f32(self.threshold) == float_key::of_f32(other.threshold)
            && float_key::of_f32(self.base) == float_key::of_f32(other.base)
            && float_key::of_f32(self.factor) == float_key::of_f32(other.factor)
    }
}

impl Eq for ItemDamage {}

impl Hash for ItemDamage {
    fn hash<H: Hasher>(&self, state: &mut H) {
        float_key::of_f32(self.threshold).hash(state);
        float_key::of_f32(self.base).hash(state);
        float_key::of_f32(self.factor).hash(state);
    }
}
