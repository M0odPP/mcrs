use mcrs_minecraft_item_stack::{
    AdditionalTradeCost, CustomName, Enchantable, EnchantmentId, Instrument, ItemName,
    OminousBottleAmplifier, PotionContents, RegistryValue, StewEffect, SuspiciousStewEffects,
};
use mcrs_minecraft_loot_condition::{LootWorld, bounded};
use mcrs_minecraft_predicate::PredicateScratch;
use mcrs_minecraft_random::Random;

use crate::modifier::{ModifierOp, StewEntry, raise};
use crate::modifier_file::NameTarget;
use crate::table::Evaluation;
use crate::work_stack::WorkStack;

pub(crate) fn apply_extra<E: Copy, W: LootWorld<E>, R: Random>(
    op: &ModifierOp,
    stack: &mut WorkStack,
    evaluation: &Evaluation<'_, '_, E, W>,
    random: &mut R,
    scratch: &mut PredicateScratch,
) {
    let (inputs, items) = (evaluation.inputs(), &evaluation.rules.items);
    match op {
        ModifierOp::EnchantWithLevels { .. } => {
            enchant_with_levels(stack, op, evaluation, random, scratch);
        }
        ModifierOp::SetName {
            name: Some(name),
            target,
            ..
        } => match target {
            NameTarget::CustomName => stack.set(CustomName(name.clone()), items),
            NameTarget::ItemName => stack.set(ItemName(name.clone()), items),
        },
        ModifierOp::SetPotion(id) => {
            let mut contents = stack
                .read()
                .and_then(|r| items.effective::<PotionContents>(r).cloned())
                .unwrap_or_default();
            contents.potion = Some(*id);
            stack.set(contents, items);
        }
        ModifierOp::SetStewEffect(entries) => set_stew(stack, entries, evaluation, random, scratch),
        ModifierOp::SetOminousBottleAmplifier(provider) => {
            let amplifier = provider.read(inputs, random, scratch).clamp(0, 4);
            stack.set(OminousBottleAmplifier(amplifier), items);
        }
        ModifierOp::SetInstrument(options) => {
            if let Some(&instrument) = pick(options, random) {
                stack.set(Instrument(RegistryValue::Reference(instrument)), items);
            }
        }
        _ => {}
    }
}

fn pick<'a, T, R: Random>(slice: &'a [T], random: &mut R) -> Option<&'a T> {
    let len = i64::try_from(slice.len()).ok()?;
    slice.get(usize::try_from(bounded(random, len)?).ok()?)
}

fn set_stew<E: Copy, W: LootWorld<E>, R: Random>(
    stack: &mut WorkStack,
    entries: &[StewEntry],
    evaluation: &Evaluation<'_, '_, E, W>,
    random: &mut R,
    scratch: &mut PredicateScratch,
) {
    let rules = evaluation.rules;
    if rules.suspicious_stew.is_none() || stack.kind() != rules.suspicious_stew {
        return;
    }
    let Some(chosen) = pick(entries, random) else {
        return;
    };
    let raw = chosen.duration.read(evaluation.inputs(), random, scratch);
    let duration = raw.saturating_mul(if chosen.instant { 1 } else { 20 });
    let mut current = stack
        .read()
        .and_then(|read| rules.items.effective::<SuspiciousStewEffects>(read))
        .map(|effects| effects.0.clone())
        .unwrap_or_default();
    current.push(StewEffect {
        effect: chosen.effect,
        duration,
    });
    stack.set(SuspiciousStewEffects(current), &rules.items);
}

#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "level cost calculation uses f32"
)]
fn enchantment_cost<R: Random>(level_value: i32, enchantability: i32, random: &mut R) -> i32 {
    let half = i64::from(enchantability / 4 + 1);
    let bonus = bounded(random, half)
        .unwrap_or(0)
        .saturating_add(bounded(random, half).unwrap_or(0));
    let factor = (random.next_f32() + random.next_f32() - 1.0_f32) * 0.15_f32;
    let base = level_value.saturating_add(1).saturating_add(bonus) as f32;
    (base + base * factor).round().max(1.0_f32) as i32
}

struct EnchantCandidate {
    id: EnchantmentId,
    level: u32,
    weight: u32,
}

fn enchant_with_levels<E: Copy, W: LootWorld<E>, R: Random>(
    stack: &mut WorkStack,
    op: &ModifierOp,
    evaluation: &Evaluation<'_, '_, E, W>,
    random: &mut R,
    scratch: &mut PredicateScratch,
) {
    let ModifierOp::EnchantWithLevels {
        levels,
        options,
        include_additional_cost_component,
    } = op
    else {
        return;
    };
    let rules = evaluation.rules;
    let level_value = levels.read(evaluation.inputs(), random, scratch);
    let kind = stack.kind();
    let is_book = kind.is_some() && kind == rules.book;
    let enchantable = stack
        .read()
        .and_then(|r| rules.items.effective::<Enchantable>(r))
        .map(|v| v.0);
    let mut results = Vec::new();
    if let (Some(kind), Some(value)) = (kind, enchantable) {
        let mut cost = enchantment_cost(level_value, value, random);
        let listed = options.as_deref().into_iter().flatten().copied();
        let every = rules
            .enchantments
            .iter()
            .map(|r| r.id)
            .filter(|_| options.is_none());
        let mut candidates: Vec<_> = listed
            .chain(every)
            .filter_map(|id| {
                let rule = rules.enchantment(id)?;
                let level = (is_book || rule.is_primary(kind))
                    .then(|| rule.level_for_cost(cost))
                    .flatten()?;
                Some(EnchantCandidate {
                    id,
                    level,
                    weight: rule.weight,
                })
            })
            .collect();
        while let Some(chosen) = weighted_pick(&candidates, random) {
            results.push((chosen.id, chosen.level));
            let last = chosen.id;
            if bounded(random, 50).unwrap_or(0) > cost {
                break;
            }
            candidates.retain(|candidate| rules.compatible(candidate.id, last));
            cost /= 2;
        }
    }
    if is_book && let Some(enchanted_book) = rules.enchanted_book {
        stack.replace(enchanted_book, 1);
    }
    for (id, level) in results {
        let level_i32 = i32::try_from(level).unwrap_or(i32::MAX);
        raise(stack, id, level_i32, evaluation);
    }
    let allow =
        *include_additional_cost_component && evaluation.context.additional_cost_component_allowed;
    if allow && stack.read().is_some() && level_value > 0 {
        stack.set(AdditionalTradeCost(level_value), &rules.items);
    }
}

fn weighted_pick<'a, R: Random>(
    candidates: &'a [EnchantCandidate],
    random: &mut R,
) -> Option<&'a EnchantCandidate> {
    let total: i64 = candidates.iter().map(|c| i64::from(c.weight)).sum();
    let mut pick = bounded(random, total)?;
    candidates.iter().find(|c| {
        pick -= i32::try_from(c.weight).unwrap_or(i32::MAX);
        pick < 0
    })
}
