use mcrs_minecraft_enchantment::enchantment_data::EnchantmentCost;
use mcrs_minecraft_enchantment::file::EnchantmentFile;
use mcrs_minecraft_item::enchantment::Enchantment;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_item_stack::{EnchantmentId, Kind};
use mcrs_minecraft_item_table::ItemTable;
use mcrs_minecraft_registries::{IdSet, Registries, Registry, Tags};
use mcrs_minecraft_resource::{Id, Registered, ResourceLocation};

#[derive(Debug, Clone)]
pub(crate) struct ItemRules {
    pub(crate) items: ItemTable,
    pub(crate) enchantments: Box<[EnchantmentRule]>,
    pub(crate) air: Option<Kind>,
    pub(crate) book: Option<Kind>,
    pub(crate) enchanted_book: Option<Kind>,
    pub(crate) suspicious_stew: Option<Kind>,
}

#[derive(Debug, Clone)]
pub(crate) struct EnchantmentRule {
    pub(crate) id: EnchantmentId,
    pub(crate) max_level: u32,
    pub(crate) supported: Box<[Kind]>,
    pub(crate) primary: Option<Box<[Kind]>>,
    pub(crate) min_cost: EnchantmentCost,
    pub(crate) max_cost: EnchantmentCost,
    pub(crate) weight: u32,
    pub(crate) exclusive: Box<[EnchantmentId]>,
}

impl EnchantmentRule {
    pub(crate) fn supports(&self, kind: Kind) -> bool {
        self.supported.binary_search(&kind).is_ok()
    }

    pub(crate) fn is_primary(&self, kind: Kind) -> bool {
        self.supports(kind)
            && self
                .primary
                .as_ref()
                .is_none_or(|primary| primary.binary_search(&kind).is_ok())
    }

    pub(crate) fn level_for_cost(&self, cost: i32) -> Option<u32> {
        let cost = u64::try_from(cost).ok()?;
        (1..=self.max_level).rev().find(|&level| {
            let step = u64::from(level.saturating_sub(1));
            let bound = |entry: &EnchantmentCost| {
                u64::from(entry.base)
                    .saturating_add(u64::from(entry.per_level_above_first).saturating_mul(step))
            };
            cost >= bound(&self.min_cost) && cost <= bound(&self.max_cost)
        })
    }
}

fn id_list<R: Registered, T: Ord>(
    set: Option<&IdSet<R>>,
    tags: Option<&Tags<R>>,
    map: fn(Id<R>) -> Option<T>,
) -> Box<[T]> {
    let mut list: Vec<T> = tags
        .into_iter()
        .flat_map(|t| set.into_iter().flat_map(move |s| s.ids(t)))
        .filter_map(map)
        .collect();
    list.sort_unstable();
    list.dedup();
    list.into_boxed_slice()
}

impl ItemRules {
    pub(crate) fn bake(registries: &Registries, items: &ItemTable) -> Self {
        let (item_tags, enchant_tags) =
            (registries.tags::<Item>(), registries.tags::<Enchantment>());
        let enchantments = registries
            .registry::<EnchantmentFile>()
            .into_iter()
            .flat_map(Registry::iter)
            .filter_map(|(id, _, file)| {
                let to_kind = |item: Id<Item>| Some(Kind(item.number()));
                let enchantment =
                    |other: Id<Enchantment>| u16::try_from(other.index()).ok().map(EnchantmentId);
                Some(EnchantmentRule {
                    id: enchantment(id)?,
                    max_level: file.max_level,
                    supported: id_list(Some(&file.supported_items), item_tags.as_ref(), to_kind),
                    primary: file
                        .primary_items
                        .as_ref()
                        .map(|s| id_list(Some(s), item_tags.as_ref(), to_kind)),
                    min_cost: file.min_cost.clone(),
                    max_cost: file.max_cost.clone(),
                    weight: file.weight,
                    exclusive: id_list(
                        file.exclusive_set.as_ref(),
                        enchant_tags.as_ref(),
                        enchantment,
                    ),
                })
            })
            .collect();
        let kind = |name: &str| {
            let name = ResourceLocation::read(name).ok()?;
            let item = registries.entries::<Item>()?.by_name(name)?;
            Some(Kind(item.number()))
        };
        Self {
            items: *items,
            enchantments,
            air: kind("minecraft:air"),
            book: kind("minecraft:book"),
            enchanted_book: kind("minecraft:enchanted_book"),
            suspicious_stew: kind("minecraft:suspicious_stew"),
        }
    }

    pub(crate) fn enchantment(&self, id: EnchantmentId) -> Option<&EnchantmentRule> {
        self.enchantments.get(usize::from(id.0))
    }

    pub(crate) fn compatible(&self, left: EnchantmentId, right: EnchantmentId) -> bool {
        left != right
            && self
                .enchantment(left)
                .is_some_and(|rule| rule.exclusive.binary_search(&right).is_err())
            && self
                .enchantment(right)
                .is_some_and(|rule| rule.exclusive.binary_search(&left).is_err())
    }

    #[cfg(test)]
    pub(crate) fn empty(items: ItemTable) -> Self {
        Self {
            items,
            enchantments: Box::new([]),
            air: None,
            book: None,
            enchanted_book: None,
            suspicious_stew: None,
        }
    }
}
