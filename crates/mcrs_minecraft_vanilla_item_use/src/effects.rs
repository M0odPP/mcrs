use bevy_ecs::change_detection::{DetectChangesMut, Mut};
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::player_state::{Effect, EffectData, PlayerFileState};
use mcrs_minecraft_engine_registries::RegistryNames;
use mcrs_minecraft_entity::mob_effect_keys::MobEffect;
use mcrs_minecraft_item_stack::{
    ConsumeEffect, EffectDetails, EffectInstance, Food, MobEffectSet, NameLookup, NameTable,
};
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_resource::{ResourceLocation, TagKey};

const MAX_FOOD_LEVEL: i32 = 20;
const ABSORPTION_HEARTS_PER_LEVEL: f32 = 4.0;
const PROBABILITY_SCALE: f32 = 1000.0;
const PROBABILITY_MODULUS: u64 = 1000;
const INFINITE_DURATION: i32 = -1;

pub(crate) fn apply_food(food: &Food, mut file_state: Mut<PlayerFileState>) {
    let next_food = file_state
        .food_level
        .saturating_add(food.nutrition)
        .clamp(0, MAX_FOOD_LEVEL);
    let next_saturation =
        (file_state.food_saturation_level + food.saturation).clamp(0.0, next_food as f32);
    file_state
        .reborrow()
        .map_unchanged(|state| &mut state.food_level)
        .set_if_neq(next_food);
    file_state
        .map_unchanged(|state| &mut state.food_saturation_level)
        .set_if_neq(next_saturation);
}

pub(crate) fn apply_consume_effects(
    effects: &[ConsumeEffect],
    seed: u64,
    names: &RegistryNames<'_>,
    registries: &CurrentRegistries,
    mut file_state: Mut<PlayerFileState>,
    mut vitals: Option<Mut<Vitals>>,
) {
    for (index, effect) in effects.iter().enumerate() {
        match effect {
            ConsumeEffect::ApplyEffects {
                effects: instances,
                probability,
            } if passes_probability(*probability, seed ^ (index as u64)) => {
                for instance in instances {
                    apply_instance(
                        instance,
                        names,
                        file_state.reborrow(),
                        vitals.as_mut().map(Mut::reborrow),
                    );
                }
            }
            ConsumeEffect::RemoveEffects(set) => {
                remove_effect_set(set, names, registries, file_state.reborrow());
            }
            ConsumeEffect::ClearAllEffects => {
                clear_all_effects(file_state.reborrow(), vitals.as_mut().map(Mut::reborrow));
            }
            ConsumeEffect::ApplyEffects { .. }
            | ConsumeEffect::TeleportRandomly { .. }
            | ConsumeEffect::PlaySound(_) => {}
        }
    }
}

fn passes_probability(probability: f32, seed: u64) -> bool {
    if !probability.is_finite() || probability <= 0.0 {
        return false;
    }
    if probability >= 1.0 {
        return true;
    }
    (seed % PROBABILITY_MODULUS) < (probability * PROBABILITY_SCALE).round() as u64
}

fn is_longer_duration(incoming: i32, existing: i32) -> bool {
    if existing == INFINITE_DURATION {
        return false;
    }
    incoming == INFINITE_DURATION || incoming > existing
}

fn should_replace_effect(instance: &EffectInstance, existing: &Effect) -> bool {
    instance.amplifier > existing.amplifier
        || (instance.amplifier == existing.amplifier
            && is_longer_duration(instance.duration, existing.duration))
}

fn apply_instance(
    instance: &EffectInstance,
    names: &RegistryNames<'_>,
    mut file_state: Mut<PlayerFileState>,
    vitals: Option<Mut<Vitals>>,
) {
    let Some(location) = names.name(NameTable::MobEffect, instance.effect.0) else {
        return;
    };
    let existing_index = file_state.active_effects.iter().position(|existing| {
        ResourceLocation::read(&existing.id).is_ok_and(|parsed| parsed == location)
    });
    if let Some(index) = existing_index {
        if let Some(existing) = file_state.active_effects.get(index)
            && should_replace_effect(instance, existing)
        {
            let incoming = to_saved_effect(location, instance);
            if let Some(slot) = file_state.active_effects.get_mut(index) {
                *slot = incoming;
            }
        }
    } else {
        let incoming = to_saved_effect(location, instance);
        file_state.active_effects.push(incoming);
    }
    if location == MobEffect::Absorption.location()
        && let Some(vitals) = vitals
    {
        let next = vitals
            .absorption
            .max(ABSORPTION_HEARTS_PER_LEVEL * f32::from(instance.amplifier.saturating_add(1)));
        vitals
            .map_unchanged(|vitals_state| &mut vitals_state.absorption)
            .set_if_neq(next);
    }
}

fn retain_non_matching(
    mut file_state: Mut<PlayerFileState>,
    should_remove: impl Fn(&Effect) -> bool,
) {
    if !file_state.active_effects.iter().any(&should_remove) {
        return;
    }
    file_state
        .active_effects
        .retain(|effect| !should_remove(effect));
}

fn remove_effect_set(
    set: &MobEffectSet,
    names: &RegistryNames<'_>,
    registries: &CurrentRegistries,
    file_state: Mut<PlayerFileState>,
) {
    if file_state.active_effects.is_empty() {
        return;
    }
    match set {
        MobEffectSet::Effects(ids) => {
            if ids.is_empty() {
                return;
            }
            retain_non_matching(file_state, |effect| {
                let Ok(effect_location) = ResourceLocation::read(&effect.id) else {
                    return false;
                };
                ids.iter()
                    .any(|id| names.name(NameTable::MobEffect, id.0) == Some(effect_location))
            });
        }
        MobEffectSet::Tag(tag_id) => {
            let (Some(tags), Some(entries), Some(tag_location)) = (
                registries.tags::<MobEffect>(),
                registries.entries::<MobEffect>(),
                names.name(NameTable::MobEffectTag, tag_id.0),
            ) else {
                return;
            };
            let Some(tag) = tags.id(TagKey::new(tag_location)) else {
                return;
            };
            retain_non_matching(file_state, |effect| {
                let Ok(effect_location) = ResourceLocation::read(&effect.id) else {
                    return false;
                };
                entries
                    .iter()
                    .find(|(_, name)| name.as_location() == effect_location)
                    .is_some_and(|(id, _)| tags.contains(tag, id))
            });
        }
    }
}

fn clear_all_effects(file_state: Mut<PlayerFileState>, vitals: Option<Mut<Vitals>>) {
    if !file_state.active_effects.is_empty() {
        file_state
            .map_unchanged(|state| &mut state.active_effects)
            .set_if_neq(Vec::new());
    }
    if let Some(vitals) = vitals {
        vitals
            .map_unchanged(|vitals_state| &mut vitals_state.absorption)
            .set_if_neq(0.0);
    }
}

fn to_saved_effect(location: ResourceLocation<'_>, instance: &EffectInstance) -> Effect {
    Effect {
        id: location.to_string(),
        amplifier: instance.amplifier,
        duration: instance.duration,
        ambient: instance.ambient,
        show_particles: instance.show_particles,
        show_icon: Some(instance.show_icon),
        hidden_effect: instance.hidden_effect.as_deref().map(to_saved_effect_data),
    }
}

fn to_saved_effect_data(details: &EffectDetails) -> Box<EffectData> {
    Box::new(EffectData {
        amplifier: details.amplifier,
        duration: details.duration,
        ambient: details.ambient,
        show_particles: details.show_particles,
        show_icon: Some(details.show_icon),
        hidden_effect: details.hidden_effect.as_deref().map(to_saved_effect_data),
    })
}

#[cfg(test)]
mod tests {
    use super::{INFINITE_DURATION, is_longer_duration};

    #[test]
    fn infinite_duration_outranks_finite_and_ties_with_infinite() {
        assert!(
            is_longer_duration(INFINITE_DURATION, 0),
            "infinite duration (-1) must outrank zero finite duration"
        );
        assert!(
            is_longer_duration(INFINITE_DURATION, 1200),
            "infinite duration (-1) must outrank positive finite duration"
        );
        assert!(
            !is_longer_duration(1200, INFINITE_DURATION),
            "finite duration must not replace infinite duration (-1)"
        );
        assert!(
            !is_longer_duration(INFINITE_DURATION, INFINITE_DURATION),
            "infinite duration (-1) must not be strictly longer than infinite duration (-1)"
        );
        assert!(
            is_longer_duration(200, 100),
            "larger finite duration must outrank shorter finite duration"
        );
        assert!(
            !is_longer_duration(100, 100),
            "equal finite durations are not strictly longer"
        );
    }
}
