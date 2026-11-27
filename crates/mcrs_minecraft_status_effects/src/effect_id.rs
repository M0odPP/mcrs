use core::mem;

#[cfg(feature = "attributes")]
use mcrs_minecraft_entity::mob_effect_keys::MobEffect;

pub const INFINITE_DURATION: i32 = -1;
pub const SYNC_INTERVAL_TICKS: i32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EffectId(pub u16);

impl EffectId {
    pub const SPEED: Self = Self(0);
    pub const STRENGTH: Self = Self(4);
    pub const INSTANT_HEALTH: Self = Self(5);
    pub const INSTANT_DAMAGE: Self = Self(6);
    pub const REGENERATION: Self = Self(9);
    pub const NIGHT_VISION: Self = Self(15);
    pub const HUNGER: Self = Self(16);
    pub const WEAKNESS: Self = Self(17);
    pub const POISON: Self = Self(18);
    pub const WITHER: Self = Self(19);
    pub const ABSORPTION: Self = Self(21);
    pub const SATURATION: Self = Self(22);
    pub const OOZING: Self = Self(37);
    pub const INFESTED: Self = Self(38);
    pub const FREEZING: Self = Self(40);

    #[cfg(feature = "attributes")]
    #[must_use]
    pub const fn to_mob_effect(self) -> Option<MobEffect> {
        MobEffect::from_protocol_id(self.0)
    }
}

#[cfg(feature = "attributes")]
impl From<MobEffect> for EffectId {
    fn from(effect: MobEffect) -> Self {
        Self(effect as u16)
    }
}

#[must_use]
pub const fn clamp_amplifier(raw: i32) -> u8 {
    match raw {
        ..=0 => 0,
        255.. => 255,
        value => value as u8,
    }
}

#[must_use]
pub const fn is_shorter_duration(first: i32, second: i32) -> bool {
    first != INFINITE_DURATION && (second == INFINITE_DURATION || first < second)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EffectImmunities(pub u8);

impl EffectImmunities {
    pub const NONE: Self = Self(0);
    pub const POISON: Self = Self(1 << 0);
    pub const WITHER: Self = Self(1 << 1);
    pub const REGENERATION: Self = Self(1 << 2);
    pub const WEAKNESS: Self = Self(1 << 3);
    pub const FREEZING: Self = Self(1 << 4);
    pub const INFESTED: Self = Self(1 << 5);
    pub const OOZING: Self = Self(1 << 6);

    #[must_use]
    pub const fn is_immune(self, effect: EffectId) -> bool {
        let bit = match effect {
            EffectId::POISON => Self::POISON.0,
            EffectId::WITHER => Self::WITHER.0,
            EffectId::REGENERATION => Self::REGENERATION.0,
            EffectId::WEAKNESS => Self::WEAKNESS.0,
            EffectId::FREEZING => Self::FREEZING.0,
            EffectId::INFESTED => Self::INFESTED.0,
            EffectId::OOZING => Self::OOZING.0,
            _ => 0,
        };
        (self.0 & bit) != 0
    }
}

pub(crate) const PTR_SIZE: usize = mem::size_of::<usize>();
pub(crate) const U64_ALIGN: usize = mem::align_of::<u64>();

pub(crate) const fn has_layout<T>(size: usize, align: usize) -> bool {
    mem::size_of::<T>() == size && mem::align_of::<T>() == align
}

const _: () = {
    let valid = has_layout::<EffectId>(2, 2) && has_layout::<EffectImmunities>(1, 1);
    assert!(valid, "EffectId and EffectImmunities layout mismatch");
};
