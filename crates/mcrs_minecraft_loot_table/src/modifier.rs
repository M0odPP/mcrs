use core::num::NonZeroU8;

use mcrs_minecraft_item::component::equippable::EquipmentSlot;
use mcrs_minecraft_item::enchantment::Enchantment;
use mcrs_minecraft_item_stack::{
    AdditionalTradeCost, Damage, EnchantmentId, EnchantmentLevel, Enchantments, InstrumentId, Kind,
    MaxDamage, MobEffectId, PotionId, StoredEnchantments, Unbreakable,
};
use mcrs_minecraft_loot_condition::{
    Condition, ContextParam, ContextParams, FloatProvider, IntProvider, LootWorld, Tool, between,
    binomial, bounded, equipment_level,
};
use mcrs_minecraft_predicate::{BakedItemPredicate, PredicateScratch};
use mcrs_minecraft_random::Random;
use mcrs_minecraft_resource::Id;
use mcrs_minecraft_text::Text;

use crate::modifier_eval;
use crate::modifier_file::NameTarget;
use crate::set_attribute::{SetAttribute, set_attributes};
use crate::table::{Evaluation, Table};
use crate::work_stack::WorkStack;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Modifier {
    pub(crate) condition: Option<Condition>,
    pub(crate) op: ModifierOp,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ModifierOp {
    Unresolved,
    Pending,
    SetCount {
        count: IntProvider,
        add: bool,
    },
    ExplosionDecay,
    Limit {
        min: Option<IntProvider>,
        max: Option<IntProvider>,
    },
    OreDrops(Option<Id<Enchantment>>),
    UniformBonus {
        enchantment: Option<Id<Enchantment>>,
        multiplier: i32,
    },
    BinomialBonus {
        enchantment: Option<Id<Enchantment>>,
        extra: i32,
        probability: f32,
    },
    Named(u32),
    EnchantedCountIncrease(CountIncrease),
    Filtered {
        filter: Box<BakedItemPredicate>,
        on_pass: Box<[Modifier]>,
        on_fail: Box<[Modifier]>,
    },
    Discard,
    SetItem(Kind),
    SetDamage {
        damage: FloatProvider,
        add: bool,
    },
    EnchantRandomly {
        options: Option<Box<[EnchantmentId]>>,
        only_compatible: bool,
        include_additional_cost_component: bool,
    },
    EnchantWithLevels {
        levels: IntProvider,
        options: Option<Box<[EnchantmentId]>>,
        include_additional_cost_component: bool,
    },
    SetEnchantments {
        levels: Box<[SetLevel]>,
        add: bool,
    },
    Sequence(Box<[Modifier]>),
    SetAttributes {
        elements: Box<[SetAttribute]>,
        replace: bool,
    },
    SetName {
        name: Option<Text>,
        entity: Option<ContextParam>,
        target: NameTarget,
    },
    SetPotion(PotionId),
    SetStewEffect(Box<[StewEntry]>),
    SetOminousBottleAmplifier(IntProvider),
    SetInstrument(Box<[InstrumentId]>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StewEntry {
    pub(crate) effect: MobEffectId,
    pub(crate) instant: bool,
    pub(crate) duration: IntProvider,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CountIncrease {
    pub(crate) enchantment: Option<Id<Enchantment>>,
    pub(crate) slots: Box<[EquipmentSlot]>,
    pub(crate) count: FloatProvider,
    pub(crate) limit: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SetLevel {
    pub(crate) enchantment: EnchantmentId,
    pub(crate) level: IntProvider,
}

impl ModifierOp {
    pub(crate) fn reads(&self) -> ContextParams {
        fn providers<'a>(providers: impl Iterator<Item = &'a IntProvider>) -> ContextParams {
            providers.fold(ContextParams::EMPTY, |reads, provider| {
                reads.union(provider.reads())
            })
        }
        match self {
            ModifierOp::Unresolved
            | ModifierOp::Pending
            | ModifierOp::Sequence(_)
            | ModifierOp::Named(_)
            | ModifierOp::Filtered { .. }
            | ModifierOp::Discard
            | ModifierOp::SetItem(_)
            | ModifierOp::SetPotion(_)
            | ModifierOp::SetInstrument(_) => ContextParams::EMPTY,
            ModifierOp::SetCount { count, .. } | ModifierOp::SetOminousBottleAmplifier(count) => {
                count.reads()
            }
            ModifierOp::SetDamage { damage, .. } => damage.reads(),
            ModifierOp::ExplosionDecay => ContextParams::of(&[ContextParam::ExplosionRadius]),
            ModifierOp::Limit { min, max } => providers([min, max].into_iter().flatten()),
            ModifierOp::OreDrops(_)
            | ModifierOp::UniformBonus { .. }
            | ModifierOp::BinomialBonus { .. } => ContextParams::of(&[ContextParam::Tool]),
            ModifierOp::EnchantedCountIncrease(increase) => {
                ContextParams::of(&[ContextParam::AttackingEntity]).union(increase.count.reads())
            }
            ModifierOp::EnchantRandomly {
                include_additional_cost_component,
                ..
            } => match include_additional_cost_component {
                true => ContextParams::of(&[ContextParam::AdditionalCostComponentAllowed]),
                false => ContextParams::EMPTY,
            },
            ModifierOp::EnchantWithLevels {
                levels,
                include_additional_cost_component,
                ..
            } => match include_additional_cost_component {
                true => levels
                    .reads()
                    .with(ContextParam::AdditionalCostComponentAllowed),
                false => levels.reads(),
            },
            ModifierOp::SetAttributes { elements, .. } => elements
                .iter()
                .fold(ContextParams::EMPTY, |reads, element| {
                    reads.union(element.amount.reads())
                }),
            ModifierOp::SetEnchantments { levels, .. } => {
                providers(levels.iter().map(|level| &level.level))
            }
            ModifierOp::SetName { entity, .. } => entity.map_or(ContextParams::EMPTY, |param| {
                ContextParams::EMPTY.with(param)
            }),
            ModifierOp::SetStewEffect(entries) => {
                providers(entries.iter().map(|entry| &entry.duration))
            }
        }
    }
}

impl Modifier {
    pub(crate) fn apply<E: Copy, W: LootWorld<E>, R: Random>(
        &self,
        holder: &Table,
        stack: &mut WorkStack,
        evaluation: &Evaluation<'_, '_, E, W>,
        random: &mut R,
        scratch: &mut PredicateScratch,
    ) {
        let inputs = evaluation.inputs();
        let context = evaluation.context;
        if self
            .condition
            .as_ref()
            .is_some_and(|condition| !condition.test(inputs, random, scratch))
        {
            return;
        }
        let count = stack.count();
        match &self.op {
            ModifierOp::Unresolved | ModifierOp::Pending => {}
            ModifierOp::SetCount { count: value, add } => {
                let value = value.read(inputs, random, scratch);
                stack.count = if *add {
                    count.saturating_add(value)
                } else {
                    value
                };
            }
            ModifierOp::ExplosionDecay => {
                if let Some(radius) = context.explosion_radius {
                    let chance = 1.0 / radius;
                    stack.count = (0..count)
                        .map(|_| i32::from(random.next_f32() <= chance))
                        .sum();
                }
            }
            ModifierOp::Limit { min, max } => {
                let floor = min.as_ref().map(|min| min.read(inputs, random, scratch));
                let ceiling = max.as_ref().map(|max| max.read(inputs, random, scratch));
                stack.count = match (floor, ceiling) {
                    (None, None) => count,
                    (None, Some(ceiling)) => count.min(ceiling),
                    (Some(floor), None) => count.max(floor),
                    (Some(floor), Some(ceiling)) => count.max(floor).min(floor.max(ceiling)),
                };
            }
            ModifierOp::OreDrops(_)
            | ModifierOp::UniformBonus { .. }
            | ModifierOp::BinomialBonus { .. } => {
                if let Some(tool) = context.tool {
                    stack.count = apply_bonus(&self.op, tool, count, random);
                }
            }
            ModifierOp::Discard => stack.discard(),
            ModifierOp::SetItem(kind) => {
                if stack.read().is_some() {
                    stack.set_kind(*kind, &evaluation.rules.items);
                }
            }
            ModifierOp::SetDamage { damage, add } => {
                set_damage(stack, damage, *add, evaluation, random, scratch)
            }
            ModifierOp::EnchantRandomly {
                options,
                only_compatible,
                include_additional_cost_component,
            } => enchant_randomly(
                stack,
                options.as_deref(),
                *only_compatible,
                *include_additional_cost_component,
                evaluation,
                random,
            ),
            ModifierOp::SetEnchantments { levels, add } => {
                set_enchantments(stack, levels, *add, evaluation, random, scratch);
            }
            ModifierOp::Sequence(modifiers) => {
                apply_all(modifiers, holder, stack, evaluation, random, scratch);
            }
            ModifierOp::Named(slot) => {
                let target = holder
                    .nested(*slot)
                    .and_then(|target| evaluation.tables.get(usize::try_from(target).ok()?));
                if let Some(target) = target {
                    let modifiers = &target.modifiers;
                    apply_all(modifiers, target, stack, evaluation, random, scratch);
                }
            }
            ModifierOp::EnchantedCountIncrease(increase) => {
                increase.apply(stack, evaluation, random, scratch);
            }
            ModifierOp::SetAttributes { elements, replace } => {
                set_attributes(stack, elements, *replace, evaluation, random, scratch);
            }
            ModifierOp::Filtered {
                filter,
                on_pass,
                on_fail,
            } => {
                let branch = match stack.passes(filter, &evaluation.rules.items) {
                    true => on_pass,
                    false => on_fail,
                };
                apply_all(branch, holder, stack, evaluation, random, scratch);
            }
            _ => modifier_eval::apply_extra(&self.op, stack, evaluation, random, scratch),
        }
    }
}

pub(crate) fn apply_all<E: Copy, W: LootWorld<E>, R: Random>(
    modifiers: &[Modifier],
    holder: &Table,
    stack: &mut WorkStack,
    evaluation: &Evaluation<'_, '_, E, W>,
    random: &mut R,
    scratch: &mut PredicateScratch,
) {
    for modifier in modifiers {
        modifier.apply(holder, stack, evaluation, random, scratch);
    }
}

impl CountIncrease {
    fn apply<E: Copy, W: LootWorld<E>, R: Random>(
        &self,
        stack: &mut WorkStack,
        evaluation: &Evaluation<'_, '_, E, W>,
        random: &mut R,
        scratch: &mut PredicateScratch,
    ) {
        let level = evaluation
            .context
            .attacking_entity
            .zip(self.enchantment)
            .map_or(0, |(attacker, enchantment)| {
                equipment_level(evaluation.world, attacker, enchantment, &self.slots)
            });
        if level > 0 {
            let per_level = self.count.read(evaluation.inputs(), random, scratch);
            stack.count = count_increase(stack.count(), level, per_level, self.limit);
        }
    }
}

fn count_increase(count: i32, level: u32, per_level: f32, limit: i32) -> i32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "the rule multiplies the level as a 32-bit float"
    )]
    let product = level as f32 * per_level;
    #[expect(clippy::cast_possible_truncation, reason = "the cast saturates")]
    let increased = count.saturating_add((product + 0.5).floor() as i32);
    match limit > 0 && increased > limit {
        true => limit,
        false => increased,
    }
}

fn apply_bonus<R: Random>(op: &ModifierOp, tool: Tool<'_>, count: i32, random: &mut R) -> i32 {
    let level = |enchantment: Option<Id<Enchantment>>| {
        let level = enchantment.map_or(0, |enchantment| tool.level(enchantment));
        i32::try_from(level).unwrap_or(i32::MAX)
    };
    match op {
        ModifierOp::OreDrops(enchantment) => {
            let level = level(*enchantment);
            if level <= 0 {
                return count;
            }
            bounded(random, i64::from(level) + 2).map_or(count, |bonus| {
                count.saturating_mul(bonus.saturating_sub(1).max(0) + 1)
            })
        }
        ModifierOp::UniformBonus {
            enchantment,
            multiplier,
        } => {
            let bound = i64::from(*multiplier) * i64::from(level(*enchantment)) + 1;
            bounded(random, bound).map_or(count, |bonus| count.saturating_add(bonus))
        }
        ModifierOp::BinomialBonus {
            enchantment,
            extra,
            probability,
        } => {
            let trials = level(*enchantment).saturating_add(*extra);
            count.saturating_add(binomial(random, trials, *probability))
        }
        _ => count,
    }
}

fn set_damage<E: Copy, W: LootWorld<E>, R: Random>(
    stack: &mut WorkStack,
    damage: &FloatProvider,
    add: bool,
    evaluation: &Evaluation<'_, '_, E, W>,
    random: &mut R,
    scratch: &mut PredicateScratch,
) {
    let items = &evaluation.rules.items;
    let Some(read) = stack.read() else {
        return;
    };
    if items.effective::<Unbreakable>(read).is_some() {
        return;
    }
    let (Some(&MaxDamage(max @ 0..)), Some(&Damage(current))) = (
        items.effective::<MaxDamage>(read),
        items.effective::<Damage>(read),
    ) else {
        return;
    };
    let value = damage.read(evaluation.inputs(), random, scratch);
    #[expect(clippy::cast_precision_loss, reason = "the rule calculates in f32")]
    let (max_f32, current_f32) = (max as f32, current.clamp(0, max) as f32);
    let base = if add {
        1.0 - current_f32 / max_f32
    } else {
        0.0
    };
    let part = 1.0 - (value + base).clamp(0.0, 1.0);
    #[expect(clippy::cast_possible_truncation, reason = "the cast saturates")]
    let written = (part * max_f32).floor() as i32;
    stack.set(Damage(written.clamp(0, max)), items);
}

fn update_enchants<E, W>(
    stack: &mut WorkStack,
    evaluation: &Evaluation<'_, '_, E, W>,
    edit: impl FnOnce(&Enchantments) -> Enchantments,
) {
    let Some(read) = stack.read() else { return };
    let items = &evaluation.rules.items;
    if Some(read.kind) == evaluation.rules.enchanted_book {
        if let Some(stored) = items.effective::<StoredEnchantments>(read) {
            stack.set(StoredEnchantments(edit(&stored.0)), items);
        }
    } else if let Some(active) = items.effective::<Enchantments>(read) {
        stack.set(edit(active), items);
    }
}

fn with_level(enchantments: &Enchantments, enchantment: EnchantmentId, level: u8) -> Enchantments {
    let others = enchantments
        .entries()
        .iter()
        .copied()
        .filter(|entry| entry.enchantment != enchantment);
    let new = NonZeroU8::new(level).map(|level| EnchantmentLevel { enchantment, level });
    Enchantments::new(others.chain(new)).unwrap_or_else(|| enchantments.clone())
}

fn level_of(enchantments: &Enchantments, enchantment: EnchantmentId) -> u8 {
    enchantments
        .entries()
        .iter()
        .find(|entry| entry.enchantment == enchantment)
        .map_or(0, |entry| entry.level.get())
}

fn enchant_randomly<E, W: LootWorld<E>, R: Random>(
    stack: &mut WorkStack,
    options: Option<&[EnchantmentId]>,
    only_compatible: bool,
    include_additional_cost_component: bool,
    evaluation: &Evaluation<'_, '_, E, W>,
    random: &mut R,
) {
    let rules = evaluation.rules;
    let kind = stack.kind();
    let is_book = kind.is_some() && kind == rules.book;
    let candidates = || {
        let listed = options.into_iter().flatten().copied();
        let every = rules
            .enchantments
            .iter()
            .map(|r| r.id)
            .filter(|_| options.is_none());
        listed.chain(every).filter(move |&id| {
            !only_compatible
                || is_book
                || kind.is_some_and(|k| rules.enchantment(id).is_some_and(|r| r.supports(k)))
        })
    };
    let length = candidates().count();
    let Some(index) = i64::try_from(length)
        .ok()
        .and_then(|length| bounded(random, length))
    else {
        return;
    };
    let Some(enchantment) = usize::try_from(index)
        .ok()
        .and_then(|index| candidates().nth(index))
    else {
        return;
    };
    let max_level = rules
        .enchantment(enchantment)
        .map_or(1, |rule| i32::try_from(rule.max_level).unwrap_or(i32::MAX));
    let level = between(random, 1, max_level).unwrap_or(1);
    if is_book && let Some(enchanted_book) = rules.enchanted_book {
        stack.replace(enchanted_book, 1);
    }
    raise(stack, enchantment, level, evaluation);
    if include_additional_cost_component && evaluation.context.additional_cost_component_allowed {
        let extra = bounded(
            random,
            i64::from(level).saturating_mul(10).saturating_add(5),
        )
        .unwrap_or(0);
        let cost = level
            .saturating_mul(3)
            .saturating_add(2)
            .saturating_add(extra);
        stack.set(AdditionalTradeCost(cost), &rules.items);
    }
}

pub(crate) fn raise<E, W: LootWorld<E>>(
    stack: &mut WorkStack,
    enchantment: EnchantmentId,
    level: i32,
    evaluation: &Evaluation<'_, '_, E, W>,
) {
    if level <= 0 {
        return;
    }
    let capped = u8::try_from(level.min(255)).unwrap_or(u8::MAX);
    update_enchants(stack, evaluation, |current| {
        with_level(
            current,
            enchantment,
            capped.max(level_of(current, enchantment)),
        )
    });
}

fn set_enchantments<E: Copy, W: LootWorld<E>, R: Random>(
    stack: &mut WorkStack,
    levels: &[SetLevel],
    add: bool,
    evaluation: &Evaluation<'_, '_, E, W>,
    random: &mut R,
    scratch: &mut PredicateScratch,
) {
    let rules = evaluation.rules;
    if stack.kind().is_some()
        && stack.kind() == rules.book
        && let Some(book) = rules.enchanted_book
    {
        stack.set_kind(book, &rules.items);
    }
    update_enchants(stack, evaluation, |current| {
        let mut enchantments = current.clone();
        for entry in levels {
            let value = entry.level.read(evaluation.inputs(), random, scratch);
            let base = if add {
                i32::from(level_of(&enchantments, entry.enchantment))
            } else {
                0
            };
            let next = u8::try_from(base.saturating_add(value).clamp(0, 255)).unwrap_or(0);
            enchantments = with_level(&enchantments, entry.enchantment, next);
        }
        enchantments
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item_rules::ItemRules;
    use mcrs_minecraft_engine_registries::{test_corpus, test_item_table};
    use mcrs_minecraft_game_registries::registry_load::test_registries;
    use mcrs_minecraft_item_stack::Builtin;
    use mcrs_minecraft_item_table::ItemTable;
    use mcrs_minecraft_loot::context_key_set_keys::ContextKeySet;
    use mcrs_minecraft_loot_condition::{LootContext, NamedEntries};
    use mcrs_minecraft_predicate::BlockHost;
    use mcrs_minecraft_random::xoroshiro::XoroshiroRandom;

    #[test]
    fn loot_mod_004_apply_bonus_without_a_tool_keeps_the_count_and_the_generator() {
        let rules = ItemRules::empty(ItemTable::leak([Vec::<Builtin>::new()]));
        let context = LootContext::<()>::try_new(ContextKeySet::Empty).expect("a source key set");
        let (named, _) =
            NamedEntries::bake(&test_corpus().0.0, test_registries(), test_item_table());
        let evaluation = Evaluation {
            tables: &[],
            named: &named,
            context: &context,
            world: &BlockHost::none(),
            rules: &rules,
        };
        let enchantment = Id::checked(0, 1);
        for op in [
            ModifierOp::OreDrops(enchantment),
            ModifierOp::UniformBonus {
                enchantment,
                multiplier: 1,
            },
            ModifierOp::BinomialBonus {
                enchantment,
                extra: 3,
                probability: 0.5,
            },
        ] {
            let modifier = Modifier {
                condition: None,
                op,
            };
            let mut random = XoroshiroRandom::new(7);
            let before = random.clone();
            let mut stack = WorkStack::one(Kind(0));
            stack.count = 2;
            let holder = Table {
                name_key: 0,
                pools: Box::new([]),
                entries: Box::new([]),
                items: Box::new([]),
                nested: Box::new([]),
                slot_sources: Box::new([]),
                modifiers: Box::new([]),
            };
            modifier.apply(
                &holder,
                &mut stack,
                &evaluation,
                &mut random,
                &mut PredicateScratch::default(),
            );
            assert_eq!((stack.count, &random), (2, &before), "{modifier:?}");
        }
    }
}
