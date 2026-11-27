use core::mem;

use glam::{DVec2, DVec3};
#[cfg(feature = "item-stack")]
use mcrs_minecraft_damage::Hit;
#[cfg(feature = "item-stack")]
use mcrs_minecraft_item_stack::{BlocksAttacks, DamageTypeSet, Weapon};

const DEGREES_TO_RADIANS: f64 = core::f64::consts::PI / 180.0;
const TICKS_PER_SECOND: f32 = 20.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BlockingUseState {
    #[default]
    Inactive,
    Active(u32),
    Disabled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct BlockingOutcome {
    pub blocked_damage: f32,
    pub remaining_damage: f32,
    pub item_damage: i32,
    pub disable_ticks: u32,
}

pub(crate) const fn fits<T>(size: usize, alignment: usize) -> bool {
    mem::size_of::<T>() == size && mem::align_of::<T>() == alignment
}

const _: () = {
    assert!(
        fits::<BlockingUseState>(8, 4),
        "BlockingUseState size 8 bytes and 4-byte alignment"
    );
    assert!(
        fits::<BlockingOutcome>(16, 4),
        "BlockingOutcome size 16 bytes and 4-byte alignment"
    );
};

const fn saturate_u32(scaled: f32) -> u32 {
    if scaled >= u32::MAX as f32 {
        u32::MAX
    } else {
        scaled as u32
    }
}

#[must_use]
pub fn horizontal_attack_angle_degrees(defender: DVec3, yaw: f32, source: Option<DVec3>) -> f32 {
    let Some(attacker) = source else { return 180.0 };
    let delta = DVec2::new(attacker.x - defender.x, attacker.z - defender.z);
    if !yaw.is_finite() || !delta.is_finite() {
        return 180.0;
    }
    let length = delta.length();
    if length <= 1.0E-4 {
        return 90.0;
    }
    let radians = f64::from(yaw) * DEGREES_TO_RADIANS;
    let cosine =
        (DVec2::new(-libm::sin(radians), libm::cos(radians)).dot(delta) / length).clamp(-1.0, 1.0);
    (libm::acos(cosine) / DEGREES_TO_RADIANS) as f32
}

#[must_use]
pub fn block_delay_ticks(seconds: f32) -> u32 {
    if seconds.is_finite() && seconds > 0.0 {
        saturate_u32(seconds * TICKS_PER_SECOND)
    } else {
        0
    }
}

#[must_use]
pub fn shield_disable_ticks(seconds: f32, scale: f32) -> u32 {
    if seconds.is_finite() && seconds > 0.0 && scale.is_finite() && scale > 0.0 {
        saturate_u32(libm::roundf(seconds * scale * TICKS_PER_SECOND))
    } else {
        0
    }
}

#[must_use]
pub fn shield_item_damage(threshold: f32, base: f32, factor: f32, blocked: f32) -> i32 {
    let valid =
        blocked.is_finite() && threshold.is_finite() && base.is_finite() && factor.is_finite();
    let raw = libm::floorf(base + factor * blocked);
    if !valid || blocked <= 0.0 || blocked < threshold || raw <= 0.0 || raw.is_nan() {
        return 0;
    }
    if raw >= i32::MAX as f32 {
        i32::MAX
    } else {
        raw as i32
    }
}

#[cfg(feature = "item-stack")]
fn set_matches(set: &DamageTypeSet, kind: u32, has_tag: &impl Fn(u16, u32) -> bool) -> bool {
    match set {
        DamageTypeSet::Tag(tag) => has_tag(tag.0, kind),
        DamageTypeSet::Types(types) => {
            u16::try_from(kind).is_ok_and(|key| types.iter().any(|entry| entry.0 == key))
        }
    }
}

#[cfg(feature = "item-stack")]
#[must_use]
pub fn evaluate_blocks_attacks(
    blocks: &BlocksAttacks,
    weapon: Option<&Weapon>,
    use_state: BlockingUseState,
    defender: DVec3,
    yaw: f32,
    hit: Hit,
    contains_tag: impl Fn(u16, u32) -> bool,
) -> BlockingOutcome {
    if !hit.amount.is_finite() || hit.amount <= 0.0 {
        return BlockingOutcome::default();
    }
    let incoming = hit.amount;
    let miss = BlockingOutcome {
        remaining_damage: incoming,
        ..Default::default()
    };
    let kind = hit.source.damage_type;
    let matches_kind = |set: &DamageTypeSet| set_matches(set, kind, &contains_tag);
    let bypassed = blocks.bypassed_by.as_ref().is_some_and(matches_kind);
    let delay = block_delay_ticks(blocks.block_delay_seconds);
    if bypassed || !matches!(use_state, BlockingUseState::Active(ticks) if ticks >= delay) {
        return miss;
    }
    let angle = horizontal_attack_angle_degrees(defender, yaw, hit.source.position);
    let mut total = 0.0_f32;
    for entry in &blocks.damage_reductions {
        let added = entry.base + entry.factor * incoming;
        let limit = entry.horizontal_blocking_angle;
        let allowed = entry.kind.as_ref().is_none_or(matches_kind);
        if limit.is_finite() && angle <= limit && allowed && added.is_finite() && added > 0.0 {
            total += added;
        }
    }
    let blocked = total.clamp(0.0, incoming);
    if blocked <= 0.0 {
        return miss;
    }
    let item = &blocks.item_damage;
    let seconds = weapon.map_or(0.0, |entry| entry.disable_blocking_for_seconds);
    BlockingOutcome {
        blocked_damage: blocked,
        remaining_damage: (incoming - blocked).max(0.0),
        item_damage: shield_item_damage(item.threshold, item.base, item.factor, blocked),
        disable_ticks: shield_disable_ticks(seconds, blocks.disable_cooldown_scale),
    }
}
