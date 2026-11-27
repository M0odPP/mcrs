use mcrs_minecraft_enchantment::file::EnchantmentFile;
use mcrs_minecraft_entity::mob_effect_keys::MobEffect;
use mcrs_minecraft_entity_attribute::ModifierOperation;
use mcrs_minecraft_entity_attribute::attribute_keys::Attribute;
use mcrs_minecraft_item::InstrumentValue;
use mcrs_minecraft_item::component::common::EquipmentSlotGroup;
use mcrs_minecraft_item::component::predicate::ItemPredicate;
use mcrs_minecraft_item::enchantment::Enchantment;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_item_stack::{
    AttributeId, EnchantmentId, InstrumentId, Kind, MobEffectId, PotionId, SlotGroup,
};
use mcrs_minecraft_loot::function::LootItemFunction;
use mcrs_minecraft_loot_condition::{
    ContextParam, FloatProviderFile, IntProvider, equipment_slots,
};
use mcrs_minecraft_nbt::{from_tag, to_nbt_tag};
use mcrs_minecraft_potion::potion_keys::Potion;
use mcrs_minecraft_registries::Named;
use mcrs_minecraft_resource::{Id, Registered, ResourceLocation, ResourceLocationBuf, TagKey};
use mcrs_minecraft_text::Text;

use crate::baker::Baker;
use crate::loot_issue::LootProblem;
use crate::loot_table_file::TagName;
use crate::modifier::{CountIncrease, Modifier, ModifierOp, SetLevel, StewEntry};
use crate::modifier_file::{
    AttributeFile, AttributeOperation, BonusFormula, EntityTarget, KeySetFile, LimitFile,
    ModifierElement, ModifierFile, ModifierKind, ModifierObject, ModifierSetFile, SlotsFile,
};
use crate::set_attribute::SetAttribute;

impl Baker<'_, '_> {
    pub(crate) fn modifiers(&mut self, file: Option<&ModifierFile>, path: &str) -> Box<[Modifier]> {
        match file {
            None => Box::new([]),
            Some(ModifierFile::Many(list)) => self.elements(list, path),
            Some(ModifierFile::One(one)) => Box::new([self.modifier(one, path)]),
            Some(ModifierFile::Key(name)) => Box::new([self.named(name, path)]),
            Some(ModifierFile::Tag(name)) => self.tagged(name, path),
        }
    }

    fn elements(&mut self, list: &[ModifierElement], path: &str) -> Box<[Modifier]> {
        list.iter()
            .enumerate()
            .map(|(number, element)| self.element(element, &format!("{path}[{number}]")))
            .collect()
    }

    fn element(&mut self, element: &ModifierElement, path: &str) -> Modifier {
        match element {
            ModifierElement::Key(name) => self.named(name, path),
            ModifierElement::Inline(one) => self.modifier(one, path),
        }
    }

    fn named(&mut self, name: &ResourceLocationBuf, path: &str) -> Modifier {
        let id = self
            .modifier_files
            .and_then(|files| files.by_name(name.as_location()));
        let Some(id) = id else {
            self.problems.push((
                path.to_owned(),
                LootProblem::UnknownModifier(name.to_string()),
            ));
            return unresolved();
        };
        let slot = self.nest(Some(self.first_modifier.saturating_add(id.index())), path);
        Modifier {
            condition: None,
            op: ModifierOp::Named(slot),
        }
    }

    fn tagged(&mut self, TagName(name): &TagName, path: &str) -> Box<[Modifier]> {
        let tags = self.registries.tags::<LootItemFunction>();
        let members: Option<Vec<usize>> = tags.as_ref().and_then(|tags| {
            let tag = tags.id(TagKey::new(name.as_location()))?;
            Some(tags.members(tag).map(Id::index).collect())
        });
        let Some(members) = members else {
            self.problems
                .push((path.to_owned(), LootProblem::UnknownTag(name.to_string())));
            return Box::new([]);
        };
        members
            .into_iter()
            .map(|member| {
                let slot = self.nest(Some(self.first_modifier.saturating_add(member)), path);
                Modifier {
                    condition: None,
                    op: ModifierOp::Named(slot),
                }
            })
            .collect()
    }

    fn modifier(&mut self, file: &ModifierObject, path: &str) -> Modifier {
        let condition = file
            .condition
            .as_ref()
            .map(|condition| self.condition(condition, &format!("{path}.condition")));
        let op = match &file.kind {
            ModifierKind::SetCount { count, add } => ModifierOp::SetCount {
                count: self.conditions.int_provider(count),
                add: *add,
            },
            ModifierKind::ExplosionDecay => ModifierOp::ExplosionDecay,
            ModifierKind::LimitCount {
                limit: LimitFile::Exact(value),
            } => {
                let value = IntProvider::constant(*value);
                ModifierOp::Limit {
                    min: Some(value.clone()),
                    max: Some(value),
                }
            }
            ModifierKind::LimitCount {
                limit: LimitFile::Range { min, max },
            } => ModifierOp::Limit {
                min: min.as_ref().map(|min| self.conditions.int_provider(min)),
                max: max.as_ref().map(|max| self.conditions.int_provider(max)),
            },
            ModifierKind::ApplyBonus {
                enchantment,
                formula,
            } => {
                let enchantment = self.enchantment(enchantment.as_location(), path);
                bonus(*formula, enchantment)
            }
            ModifierKind::EnchantedCountIncrease {
                enchantment,
                count,
                limit,
            } => self.enchanted_count_increase(enchantment, count, *limit, path),
            ModifierKind::SetAttributes { modifiers, replace } => {
                self.set_attributes(modifiers, *replace, path)
            }
            ModifierKind::Filtered {
                item_filter,
                on_pass,
                on_fail,
            } => self.filtered(item_filter, on_pass.as_deref(), on_fail.as_deref(), path),
            ModifierKind::Discard => ModifierOp::Discard,
            ModifierKind::SetItem { item } => self.set_item(item, path),
            ModifierKind::SetDamage { damage, add } => ModifierOp::SetDamage {
                damage: self.conditions.float_provider(damage),
                add: *add,
            },
            kind => self.modifier_extra(kind, path),
        };
        self.take_problems(path);
        self.check(op.reads(), path);
        Modifier { condition, op }
    }

    fn modifier_extra(&mut self, kind: &ModifierKind, path: &str) -> ModifierOp {
        match kind {
            ModifierKind::EnchantRandomly {
                options,
                only_compatible,
                include_additional_cost_component,
            } => ModifierOp::EnchantRandomly {
                options: options.as_ref().map(|o| {
                    self.key_set(o, "enchantment", &format!("{path}.options"), enchantment_id)
                }),
                only_compatible: *only_compatible,
                include_additional_cost_component: *include_additional_cost_component,
            },
            ModifierKind::EnchantWithLevels {
                levels,
                options,
                include_additional_cost_component,
            } => ModifierOp::EnchantWithLevels {
                levels: self.conditions.int_provider(levels),
                options: options.as_ref().map(|o| {
                    self.key_set(o, "enchantment", &format!("{path}.options"), enchantment_id)
                }),
                include_additional_cost_component: *include_additional_cost_component,
            },
            ModifierKind::SetEnchantments { enchantments, add } => {
                let levels = enchantments
                    .0
                    .iter()
                    .filter_map(|entry| {
                        let level = self.conditions.int_provider(&entry.number);
                        let enchantment = self.enchantment(entry.key.as_location(), path)?;
                        Some(SetLevel {
                            enchantment: enchantment_id(enchantment)?,
                            level,
                        })
                    })
                    .collect();
                ModifierOp::SetEnchantments { levels, add: *add }
            }
            ModifierKind::Sequence { functions } => {
                let path = format!("{path}.functions");
                ModifierOp::Sequence(match functions {
                    ModifierSetFile::Many(list) => self.elements(list, &path),
                    ModifierSetFile::One(element) => Box::new([self.element(element, &path)]),
                    ModifierSetFile::Tag(name) => self.tagged(name, &path),
                })
            }
            ModifierKind::SetName {
                name,
                entity,
                target,
            } => ModifierOp::SetName {
                name: name
                    .as_ref()
                    .and_then(|t| self.bake_text(t, &format!("{path}.name"))),
                entity: entity.map(entity_param),
                target: *target,
            },
            ModifierKind::SetStewEffect { effects } => self.set_stew_effect(effects, path),
            ModifierKind::SetOminousBottleAmplifier { amplifier } => {
                ModifierOp::SetOminousBottleAmplifier(self.conditions.int_provider(amplifier))
            }
            ModifierKind::SetPotion { id } => {
                match self
                    .checked_lookup::<Potion>(id.as_location(), "potion", &format!("{path}.id"))
                    .and_then(|id| u16::try_from(id.index()).ok())
                {
                    Some(number) => ModifierOp::SetPotion(PotionId(number)),
                    None => ModifierOp::Unresolved,
                }
            }
            ModifierKind::SetInstrument { options } => {
                ModifierOp::SetInstrument(self.key_set::<InstrumentValue, InstrumentId>(
                    options,
                    "instrument",
                    &format!("{path}.options"),
                    |id| u16::try_from(id.index()).ok().map(InstrumentId),
                ))
            }
            kind => self.pending(kind, path),
        }
    }

    fn set_stew_effect(
        &mut self,
        effects: &[crate::modifier_file::StewEffectFile],
        path: &str,
    ) -> ModifierOp {
        let entries = effects
            .iter()
            .enumerate()
            .filter_map(|(i, effect)| {
                let id = self.checked_lookup::<MobEffect>(
                    effect.effect.as_location(),
                    "mob_effect",
                    &format!("{path}.effects[{i}].type"),
                )?;
                let instant = id == MobEffect::InstantHealth.id()
                    || id == MobEffect::InstantDamage.id()
                    || id == MobEffect::Saturation.id();
                Some(StewEntry {
                    effect: MobEffectId(u16::try_from(id.index()).ok()?),
                    instant,
                    duration: self.conditions.int_provider(&effect.duration),
                })
            })
            .collect();
        ModifierOp::SetStewEffect(entries)
    }

    fn pending(&mut self, kind: &ModifierKind, path: &str) -> ModifierOp {
        match kind {
            ModifierKind::SetContents { entries, .. } => {
                self.entries(entries, None, &format!("{path}.entries"));
            }
            ModifierKind::ModifyContents { modifier, .. } => {
                self.modifiers(Some(modifier), &format!("{path}.modifier"));
            }
            ModifierKind::SetLootTable { loot_table_id, .. } => {
                self.table_key(loot_table_id, &format!("{path}.loot_table_id"));
            }
            _ => {}
        }
        ModifierOp::Pending
    }

    fn set_item(&mut self, item: &ResourceLocationBuf, path: &str) -> ModifierOp {
        if is_air(item) {
            self.problems
                .push((path.to_owned(), LootProblem::SetItemAir));
            return ModifierOp::Unresolved;
        }
        match self.lookup::<Item>(item.as_location()) {
            Some(item) => ModifierOp::SetItem(Kind(item.number())),
            None => {
                self.problems
                    .push((path.to_owned(), LootProblem::UnknownItem(item.to_string())));
                ModifierOp::Unresolved
            }
        }
    }

    fn enchanted_count_increase(
        &mut self,
        enchantment: &ResourceLocationBuf,
        count: &FloatProviderFile,
        limit: i32,
        path: &str,
    ) -> ModifierOp {
        let enchantment = self.enchantment(enchantment.as_location(), path);
        let slots = enchantment
            .and_then(|id| self.registries.registry::<EnchantmentFile>()?.get(id))
            .map(|file| equipment_slots(&file.slots))
            .unwrap_or_default();
        ModifierOp::EnchantedCountIncrease(CountIncrease {
            enchantment,
            slots,
            count: self.conditions.float_provider(count),
            limit,
        })
    }

    fn set_attributes(&mut self, files: &[AttributeFile], replace: bool, path: &str) -> ModifierOp {
        let elements: Option<Box<[SetAttribute]>> = files
            .iter()
            .enumerate()
            .map(|(number, file)| self.set_attribute(file, &format!("{path}.modifiers[{number}]")))
            .collect();
        elements.map_or(ModifierOp::Unresolved, |elements| {
            ModifierOp::SetAttributes { elements, replace }
        })
    }

    fn set_attribute(&mut self, file: &AttributeFile, path: &str) -> Option<SetAttribute> {
        let amount = self.conditions.float_provider(&file.amount);
        let attribute = self
            .lookup::<Attribute>(file.attribute.as_location())
            .and_then(|attribute| u16::try_from(attribute.index()).ok());
        let Some(attribute) = attribute else {
            let name = file.attribute.to_string();
            self.problems
                .push((path.to_owned(), LootProblem::UnknownAttribute(name)));
            return None;
        };
        let slots: Box<[SlotGroup]> = match &file.slot {
            SlotsFile::Many(groups) => groups.iter().copied().map(slot_group).collect(),
            SlotsFile::One(group) => Box::new([slot_group(*group)]),
        };
        Some(SetAttribute {
            attribute: AttributeId(attribute),
            id: file.id.clone(),
            amount,
            operation: match file.operation {
                AttributeOperation::Value => ModifierOperation::AddValue,
                AttributeOperation::MultipliedBase => ModifierOperation::AddMultipliedBase,
                AttributeOperation::MultipliedTotal => ModifierOperation::AddMultipliedTotal,
            },
            slots,
        })
    }

    fn filtered(
        &mut self,
        filter: &ItemPredicate<Named>,
        on_pass: Option<&ModifierFile>,
        on_fail: Option<&ModifierFile>,
        path: &str,
    ) -> ModifierOp {
        let filter = self.conditions.item_predicate(filter);
        self.take_problems(&format!("{path}.item_filter"));
        let on_pass = self.modifiers(on_pass, &format!("{path}.on_pass"));
        let on_fail = self.modifiers(on_fail, &format!("{path}.on_fail"));
        match filter {
            Some(filter) => ModifierOp::Filtered {
                filter: Box::new(filter),
                on_pass,
                on_fail,
            },
            None => ModifierOp::Unresolved,
        }
    }

    fn key_set<R: Registered, T>(
        &mut self,
        options: &KeySetFile,
        kind: &'static str,
        path: &str,
        map: fn(Id<R>) -> Option<T>,
    ) -> Box<[T]> {
        match options {
            KeySetFile::Tag(TagName(name)) => {
                let members: Option<Box<[T]>> = self.registries.tags::<R>().and_then(|tags| {
                    let tag = tags.id(TagKey::new(name.as_location()))?;
                    Some(tags.members(tag).filter_map(map).collect())
                });
                members.unwrap_or_else(|| {
                    self.problems
                        .push((path.to_owned(), LootProblem::UnknownTag(name.to_string())));
                    Box::new([])
                })
            }
            KeySetFile::Many(names) => names
                .iter()
                .filter_map(|name| self.checked_lookup::<R>(name.as_location(), kind, path))
                .filter_map(map)
                .collect(),
            KeySetFile::One(name) => self
                .checked_lookup::<R>(name.as_location(), kind, path)
                .and_then(map)
                .into_iter()
                .collect(),
        }
    }

    fn checked_lookup<R: Registered>(
        &mut self,
        name: ResourceLocation<'_>,
        kind: &'static str,
        path: &str,
    ) -> Option<Id<R>> {
        let id = self.lookup::<R>(name);
        if id.is_none() {
            let problem = if kind == "enchantment" {
                LootProblem::UnknownEnchantment(name.to_string())
            } else {
                LootProblem::UnknownEntry(kind, name.to_string())
            };
            self.problems.push((path.to_owned(), problem));
        }
        id
    }

    fn bake_text(&mut self, text: &mcrs_minecraft_item::Text<Named>, path: &str) -> Option<Text> {
        let tag = to_nbt_tag(text).ok();
        tag.and_then(|tag| from_tag::<Text>(tag).ok()).or_else(|| {
            self.problems.push((
                path.to_owned(),
                LootProblem::InvalidComponent("text".to_owned()),
            ));
            None
        })
    }

    pub(crate) fn enchantment(
        &mut self,
        name: ResourceLocation<'_>,
        path: &str,
    ) -> Option<Id<Enchantment>> {
        self.checked_lookup::<Enchantment>(name, "enchantment", path)
    }
}

fn entity_param(target: EntityTarget) -> ContextParam {
    match target {
        EntityTarget::This => ContextParam::ThisEntity,
        EntityTarget::Attacker => ContextParam::AttackingEntity,
        EntityTarget::DirectAttacker => ContextParam::DirectAttackingEntity,
        EntityTarget::AttackingPlayer => ContextParam::LastDamagePlayer,
        EntityTarget::TargetEntity => ContextParam::TargetEntity,
        EntityTarget::InteractingEntity => ContextParam::InteractingEntity,
    }
}

fn unresolved() -> Modifier {
    Modifier {
        condition: None,
        op: ModifierOp::Unresolved,
    }
}

fn bonus(formula: BonusFormula, enchantment: Option<Id<Enchantment>>) -> ModifierOp {
    match formula {
        BonusFormula::OreDrops { .. } => ModifierOp::OreDrops(enchantment),
        BonusFormula::UniformBonusCount { parameters } => ModifierOp::UniformBonus {
            enchantment,
            multiplier: parameters.bonus_multiplier,
        },
        BonusFormula::BinomialWithBonusCount { parameters } => ModifierOp::BinomialBonus {
            enchantment,
            extra: parameters.extra,
            probability: parameters.probability,
        },
    }
}

fn slot_group(group: EquipmentSlotGroup) -> SlotGroup {
    match group {
        EquipmentSlotGroup::Any => SlotGroup::Any,
        EquipmentSlotGroup::Mainhand => SlotGroup::Mainhand,
        EquipmentSlotGroup::Offhand => SlotGroup::Offhand,
        EquipmentSlotGroup::Hand => SlotGroup::Hand,
        EquipmentSlotGroup::Feet => SlotGroup::Feet,
        EquipmentSlotGroup::Legs => SlotGroup::Legs,
        EquipmentSlotGroup::Chest => SlotGroup::Chest,
        EquipmentSlotGroup::Head => SlotGroup::Head,
        EquipmentSlotGroup::Armor => SlotGroup::Armor,
        EquipmentSlotGroup::Body => SlotGroup::Body,
        EquipmentSlotGroup::Saddle => SlotGroup::Saddle,
    }
}

pub(crate) fn is_air(item: &ResourceLocationBuf) -> bool {
    item.namespace() == "minecraft" && item.path() == "air"
}

fn enchantment_id(id: Id<Enchantment>) -> Option<EnchantmentId> {
    u16::try_from(id.index()).ok().map(EnchantmentId)
}
