#[cfg(feature = "item-stack")]
use core::mem;

#[cfg(feature = "item-stack")]
use mcrs_minecraft_damage::{DamageTags, DamageTypeRules, HitOutcome, HitReport, Vitals};
#[cfg(feature = "item-stack")]
use mcrs_minecraft_item_stack::{ConsumeEffect, DeathProtection};

pub const TOTEM_ENTITY_EVENT: u8 = 35;
pub const SURVIVAL_HEALTH: f32 = 1.0;

#[cfg(feature = "item-stack")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeathProtectionEffectsOutcome<'a> {
    pub triggered: bool,
    pub entity_event: Option<u8>,
    pub death_effects: &'a [ConsumeEffect],
}

#[cfg(feature = "item-stack")]
const _: () = {
    const SIZE: usize = mem::size_of::<&[ConsumeEffect]>() + mem::align_of::<&[ConsumeEffect]>();
    const ALIGNMENT: usize = mem::align_of::<&[ConsumeEffect]>();
    assert!(
        mem::size_of::<DeathProtectionEffectsOutcome<'_>>() == SIZE
            && mem::align_of::<DeathProtectionEffectsOutcome<'_>>() == ALIGNMENT,
        "DeathProtectionEffectsOutcome matches slice size and alignment"
    );
};

#[cfg(feature = "item-stack")]
#[must_use]
pub fn evaluate_death_protection_component<'a>(
    protection: &'a DeathProtection,
    vitals: &mut Vitals,
    report: &mut HitReport,
    damage_type: u32,
    types: &DamageTypeRules,
) -> DeathProtectionEffectsOutcome<'a> {
    let lethal = report.died || !vitals.is_alive() || vitals.health <= 0.0;
    let bypass = DamageTags::BYPASSES_INVULNERABILITY;
    if report.outcome == HitOutcome::Rejected || !lethal || types.tags(damage_type).contains(bypass)
    {
        return Default::default();
    }
    vitals.health = SURVIVAL_HEALTH;
    vitals.death_time = None;
    report.died = false;
    DeathProtectionEffectsOutcome {
        triggered: true,
        entity_event: Some(TOTEM_ENTITY_EVENT),
        death_effects: &protection.death_effects,
    }
}
