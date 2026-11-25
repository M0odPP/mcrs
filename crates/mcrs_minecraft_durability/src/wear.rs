use libm::floorf;
use mcrs_minecraft_entity_key::KeyedStream;

use crate::ArmourBypass;
use crate::HurtWearPolicy;
use crate::WearKind;

const MAX_EXACT_I32_PLUS_ONE: f32 = 2_147_483_648.0;

#[must_use]
pub const fn block_break_wear(damage_per_block: i32, block_hardness: f32) -> i32 {
    if damage_per_block <= 0 || !block_hardness.is_finite() || block_hardness <= 0.0 {
        return 0;
    }
    damage_per_block
}

#[must_use]
pub const fn attack_wear(item_damage_per_attack: i32) -> i32 {
    if item_damage_per_attack <= 0 {
        return 0;
    }
    item_damage_per_attack
}

#[must_use]
pub fn armour_hit_wear(raw_amount: f32, hurt_policy: HurtWearPolicy, bypass: ArmourBypass) -> i32 {
    if matches!(bypass, ArmourBypass::BypassesArmour)
        || matches!(hurt_policy, HurtWearPolicy::IgnoresHurt)
        || !raw_amount.is_finite()
        || raw_amount <= 0.0
    {
        return 0;
    }
    let scaled = floorf(raw_amount / 4.0);
    if scaled >= MAX_EXACT_I32_PLUS_ONE {
        return i32::MAX;
    }
    (scaled as i32).max(1)
}

#[must_use]
pub fn roll_unbreaking(
    amount: i32,
    unbreaking_level: i32,
    kind: WearKind,
    stream: &KeyedStream,
) -> i32 {
    roll_unbreaking_salted(amount, unbreaking_level, kind, stream, 0)
}

#[must_use]
pub fn roll_unbreaking_salted(
    amount: i32,
    unbreaking_level: i32,
    kind: WearKind,
    stream: &KeyedStream,
    salt: u64,
) -> i32 {
    if amount <= 0 {
        return 0;
    }
    if unbreaking_level <= 0 {
        return amount;
    }
    let Ok(units) = u32::try_from(amount) else {
        return 0;
    };
    let denom = f64::from(unbreaking_level) + 1.0;
    let keep_chance = match kind {
        WearKind::Tool => 1.0 / denom,
        WearKind::Armour => 0.6 + 0.4 / denom,
    };
    let mut kept = 0_i32;
    for draw in 0..u64::from(units) {
        if stream.unit(salt.wrapping_add(draw)) < keep_chance {
            kept = kept.saturating_add(1);
        }
    }
    kept
}
