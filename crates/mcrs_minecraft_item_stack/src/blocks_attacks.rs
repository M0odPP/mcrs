use alloc::vec;
use alloc::vec::Vec;
use core::hash::Hash;
use core::hash::Hasher;
use core::mem::align_of;
use core::mem::size_of;

use crate::DamageReduction;
use crate::DamageTypeSet;
use crate::ItemDamage;
use crate::SoundEventRef;
use crate::float_key;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct BlocksAttacks {
    pub block_delay_seconds: f32,
    pub disable_cooldown_scale: f32,
    pub damage_reductions: Vec<DamageReduction>,
    pub item_damage: ItemDamage,
    pub bypassed_by: Option<DamageTypeSet>,
    pub block_sound: Option<SoundEventRef>,
    pub disabled_sound: Option<SoundEventRef>,
}

const _: () = {
    assert!(
        size_of::<BlocksAttacks>()
            >= size_of::<Vec<DamageReduction>>()
                + size_of::<Option<DamageTypeSet>>()
                + 2 * size_of::<Option<SoundEventRef>>()
                + 20,
        "BlocksAttacks layout matches expected fields"
    );
    assert!(
        align_of::<BlocksAttacks>() == align_of::<Option<SoundEventRef>>(),
        "BlocksAttacks aligns with pointer fields"
    );
};

impl BlocksAttacks {
    pub const DEFAULT_BLOCK_DELAY_SECONDS: f32 = 0.0;
    pub const DEFAULT_DISABLE_COOLDOWN_SCALE: f32 = 1.0;

    #[must_use]
    pub fn new() -> Self {
        Self {
            block_delay_seconds: Self::DEFAULT_BLOCK_DELAY_SECONDS,
            disable_cooldown_scale: Self::DEFAULT_DISABLE_COOLDOWN_SCALE,
            damage_reductions: vec![DamageReduction::default()],
            item_damage: ItemDamage::default(),
            bypassed_by: None,
            block_sound: None,
            disabled_sound: None,
        }
    }

    pub(crate) fn has_default_reductions(&self) -> bool {
        matches!(self.damage_reductions.as_slice(), [only] if only.is_default_exact())
    }
}

impl Default for BlocksAttacks {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for BlocksAttacks {
    fn eq(&self, other: &Self) -> bool {
        float_key::of_f32(self.block_delay_seconds) == float_key::of_f32(other.block_delay_seconds)
            && float_key::of_f32(self.disable_cooldown_scale)
                == float_key::of_f32(other.disable_cooldown_scale)
            && self.damage_reductions == other.damage_reductions
            && self.item_damage == other.item_damage
            && self.bypassed_by == other.bypassed_by
            && self.block_sound == other.block_sound
            && self.disabled_sound == other.disabled_sound
    }
}

impl Eq for BlocksAttacks {}

impl Hash for BlocksAttacks {
    fn hash<H: Hasher>(&self, state: &mut H) {
        float_key::of_f32(self.block_delay_seconds).hash(state);
        float_key::of_f32(self.disable_cooldown_scale).hash(state);
        self.damage_reductions.hash(state);
        self.item_damage.hash(state);
        self.bypassed_by.hash(state);
        self.block_sound.hash(state);
        self.disabled_sound.hash(state);
    }
}
