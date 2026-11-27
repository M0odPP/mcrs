use glam::DVec3;
use mcrs_minecraft_item_stack::ItemStack;
use mcrs_minecraft_loot::context_key_set_keys::ContextKeySet;
use mcrs_minecraft_nbt::compound::NbtCompound;
use mcrs_minecraft_predicate::{DamageSource, EntityView, ItemSubject};
use mcrs_voxel::BlockStateId;

use crate::block_entity_view::BlockEntityView;
use crate::context_param::ContextParam;
use crate::context_params::ContextParams;
use crate::dynamic_name::DynamicName;
use crate::loot_container::LootContainer;
use crate::tool::Tool;

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct LootContext<'a, E> {
    pub origin: Option<DVec3>,
    pub this_entity: Option<E>,
    pub attacking_entity: Option<E>,
    pub direct_attacking_entity: Option<E>,
    pub last_damage_player: Option<E>,
    pub target_entity: Option<E>,
    pub interacting_entity: Option<E>,
    pub tool: Option<Tool<'a>>,
    pub block_state: Option<BlockStateId>,
    pub block_entity: Option<&'a NbtCompound>,
    pub explosion_radius: Option<f32>,
    pub enchantment_level: Option<i32>,
    pub damage_source: Option<DamageSource<E>>,
    pub enchantment_active: Option<bool>,
    pub additional_cost_component_allowed: bool,
    pub container: Option<LootContainer<'a, E>>,
    pub luck: f32,
    pub sherds: &'a [Option<ItemStack>],
    pub contents: &'a [Option<ItemStack>],
    key_set: ContextKeySet,
}

impl<'a, E> LootContext<'a, E> {
    #[must_use]
    pub const fn try_new(key_set: ContextKeySet) -> Option<Self> {
        if matches!(key_set, ContextKeySet::Generic) {
            return None;
        }
        Some(Self {
            origin: None,
            this_entity: None,
            attacking_entity: None,
            direct_attacking_entity: None,
            last_damage_player: None,
            target_entity: None,
            interacting_entity: None,
            tool: None,
            block_state: None,
            block_entity: None,
            explosion_radius: None,
            enchantment_level: None,
            damage_source: None,
            enchantment_active: None,
            additional_cost_component_allowed: false,
            container: None,
            luck: 0.0,
            sherds: &[],
            contents: &[],
            key_set,
        })
    }

    #[must_use]
    pub const fn dynamic_name(&self, name: DynamicName) -> &'a [Option<ItemStack>] {
        match name {
            DynamicName::Sherds => self.sherds,
            DynamicName::Contents => self.contents,
        }
    }

    #[must_use]
    pub const fn entity(&self, param: ContextParam) -> Option<&E> {
        match param {
            ContextParam::ThisEntity => self.this_entity.as_ref(),
            ContextParam::AttackingEntity => self.attacking_entity.as_ref(),
            ContextParam::DirectAttackingEntity => self.direct_attacking_entity.as_ref(),
            ContextParam::LastDamagePlayer => self.last_damage_player.as_ref(),
            ContextParam::TargetEntity => self.target_entity.as_ref(),
            ContextParam::InteractingEntity => self.interacting_entity.as_ref(),
            _ => None,
        }
    }

    #[must_use]
    pub fn slot<'w, W: EntityView<Entity = E> + BlockEntityView + ?Sized>(
        &self,
        param: ContextParam,
        world: &'w W,
        slot: u16,
    ) -> Option<ItemSubject<'w>>
    where
        E: Copy,
        'a: 'w,
    {
        if let Some(&entity) = self.entity(param) {
            return world.slot(entity, slot);
        }
        match param {
            ContextParam::BlockEntity => self
                .block_entity
                .and_then(|_| world.block_entity_slot(slot)),
            ContextParam::Container => self
                .container
                .and_then(|container| container.slot(world, slot)),
            ContextParam::Tool => (slot == 0)
                .then_some(self.tool)
                .flatten()
                .map(ItemSubject::from),
            _ => None,
        }
    }

    #[must_use]
    pub fn held(&self) -> ContextParams {
        let Self {
            origin,
            this_entity,
            attacking_entity,
            direct_attacking_entity,
            last_damage_player,
            target_entity,
            interacting_entity,
            tool,
            block_state,
            block_entity,
            explosion_radius,
            enchantment_level,
            damage_source,
            enchantment_active,
            additional_cost_component_allowed,
            container,
            luck: _,
            sherds: _,
            contents: _,
            key_set: _,
        } = self;
        [
            (origin.is_some(), ContextParam::Origin),
            (this_entity.is_some(), ContextParam::ThisEntity),
            (attacking_entity.is_some(), ContextParam::AttackingEntity),
            (
                direct_attacking_entity.is_some(),
                ContextParam::DirectAttackingEntity,
            ),
            (last_damage_player.is_some(), ContextParam::LastDamagePlayer),
            (target_entity.is_some(), ContextParam::TargetEntity),
            (
                interacting_entity.is_some(),
                ContextParam::InteractingEntity,
            ),
            (tool.is_some(), ContextParam::Tool),
            (block_state.is_some(), ContextParam::BlockState),
            (block_entity.is_some(), ContextParam::BlockEntity),
            (explosion_radius.is_some(), ContextParam::ExplosionRadius),
            (enchantment_level.is_some(), ContextParam::EnchantmentLevel),
            (damage_source.is_some(), ContextParam::DamageSource),
            (
                enchantment_active.is_some(),
                ContextParam::EnchantmentActive,
            ),
            (
                *additional_cost_component_allowed,
                ContextParam::AdditionalCostComponentAllowed,
            ),
            (container.is_some(), ContextParam::Container),
        ]
        .into_iter()
        .filter(|&(present, _)| present)
        .fold(ContextParams::EMPTY, |held, (_, param)| held.with(param))
    }

    #[must_use]
    pub const fn key_set(&self) -> ContextKeySet {
        self.key_set
    }
}
