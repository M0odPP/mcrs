use mcrs_minecraft_entity::entity_type_keys::EntityType;
use mcrs_minecraft_entity::entity_type_tag_keys;
use mcrs_minecraft_entity::mob_effect_keys::MobEffect;
use mcrs_minecraft_entity_attribute::attribute_keys::Attribute;
use mcrs_minecraft_entity_attribute::{
    AttributeInstances, AttributeModifier, InstanceChange, InstanceEdit, InstanceModifier,
    ModifierKind, ModifierOperation,
};
use mcrs_minecraft_resource::{ResourceLocation, TagKey, rl};

type ModifierId = ResourceLocation<'static>;

const HIDE_ID: ModifierId = rl!("minecraft:effect.waypoint_transmit_range_hide");

const fn value(attribute: Attribute, id: ModifierId, base: f32) -> EffectAttributeModifier {
    EffectAttributeModifier {
        attribute,
        modifier: AttributeModifier {
            id,
            amount: base as f64,
            operation: ModifierOperation::AddValue,
        },
    }
}

const fn total(attribute: Attribute, id: ModifierId, base: f32) -> EffectAttributeModifier {
    EffectAttributeModifier {
        attribute,
        modifier: AttributeModifier {
            id,
            amount: base as f64,
            operation: ModifierOperation::AddMultipliedTotal,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectAttributeModifier {
    pub attribute: Attribute,
    pub modifier: AttributeModifier<'static>,
}

impl EffectAttributeModifier {
    #[must_use]
    pub fn to_instance_modifier(self) -> InstanceModifier {
        InstanceModifier::new(
            self.modifier.id.to_buf(),
            self.modifier.amount,
            self.modifier.operation,
            ModifierKind::Permanent,
        )
    }
}

#[must_use]
pub const fn effect_attribute_modifier(
    effect: MobEffect,
    amplifier: u8,
) -> Option<EffectAttributeModifier> {
    use Attribute::{
        AttackDamage, AttackSpeed, MaxAbsorption, MaxHealth, MovementSpeed, SafeFallDistance,
        WaypointTransmitRange,
    };
    use MobEffect::*;
    let mut spec = match effect {
        Speed => total(MovementSpeed, rl!("minecraft:effect.speed"), 0.2),
        Slowness => total(MovementSpeed, rl!("minecraft:effect.slowness"), -0.15),
        Haste => total(AttackSpeed, rl!("minecraft:effect.haste"), 0.1),
        MiningFatigue => total(AttackSpeed, rl!("minecraft:effect.mining_fatigue"), -0.1),
        Strength => value(AttackDamage, rl!("minecraft:effect.strength"), 3.0),
        Weakness => value(AttackDamage, rl!("minecraft:effect.weakness"), -4.0),
        JumpBoost => value(SafeFallDistance, rl!("minecraft:effect.jump_boost"), 1.0),
        HealthBoost => value(MaxHealth, rl!("minecraft:effect.health_boost"), 4.0),
        Absorption => value(MaxAbsorption, rl!("minecraft:effect.absorption"), 4.0),
        Luck => value(Attribute::Luck, rl!("minecraft:effect.luck"), 1.0),
        Unluck => value(Attribute::Luck, rl!("minecraft:effect.unluck"), -1.0),
        Invisibility => total(WaypointTransmitRange, HIDE_ID, -1.0),
        _ => return None,
    };
    spec.modifier.amount *= (amplifier as u16 + 1) as f64;
    Some(spec)
}

pub fn apply_effect_attribute_modifier(
    instances: &mut AttributeInstances,
    effect: MobEffect,
    amplifier: u8,
) -> InstanceChange {
    let Some(spec) = effect_attribute_modifier(effect, amplifier) else {
        return InstanceChange::Unchanged;
    };
    let edit = InstanceEdit::Put(spec.to_instance_modifier());
    instances.apply(spec.attribute, &edit)
}

pub fn remove_effect_attribute_modifier(
    instances: &mut AttributeInstances,
    effect: MobEffect,
) -> InstanceChange {
    let Some(spec) = effect_attribute_modifier(effect, 0) else {
        return InstanceChange::Unchanged;
    };
    let edit = InstanceEdit::Remove(spec.modifier.id.to_buf());
    instances.apply(spec.attribute, &edit)
}

#[must_use]
pub const fn effect_immunity_tag(effect: MobEffect) -> Option<TagKey<'static, EntityType>> {
    match effect {
        MobEffect::Poison => Some(entity_type_tag_keys::IMMUNE_TO_POISON),
        MobEffect::Wither => Some(entity_type_tag_keys::IMMUNE_TO_WITHER),
        MobEffect::Regeneration => Some(entity_type_tag_keys::IMMUNE_TO_REGENERATION),
        MobEffect::Weakness => Some(entity_type_tag_keys::IMMUNE_TO_WEAKNESS),
        MobEffect::Freezing => Some(entity_type_tag_keys::IMMUNE_TO_FREEZING),
        MobEffect::Infested => Some(entity_type_tag_keys::IMMUNE_TO_INFESTED),
        MobEffect::Oozing => Some(entity_type_tag_keys::IMMUNE_TO_OOZING),
        _ => None,
    }
}
