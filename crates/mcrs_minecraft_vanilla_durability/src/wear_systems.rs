use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::entity::Entity;
use bevy_ecs::message;
use bevy_ecs::system;
use mcrs_minecraft_damage as damage;
use mcrs_minecraft_durability::{
    self as durability, ArmourBypass::AppliesToArmour, HurtWearPolicy,
};
use mcrs_minecraft_engine::bus;
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine_damage as engine_damage;
use mcrs_minecraft_engine_item::{self as engine_item, StackCommandsExt, effective};
use mcrs_minecraft_engine_world as engine_world;
use mcrs_minecraft_entity_key as entity_key;
use mcrs_minecraft_inventory_player::ArmourSlot;
use mcrs_minecraft_item_stack as item_stack;
use mcrs_minecraft_protocol_game::clientbound::ClientboundEntityEvent;
use mcrs_minecraft_replication::MobSignal;

use crate::DurabilitySlot;

#[derive(bevy_ecs::resource::Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct UnbreakingEnchantment(pub(crate) Option<item_stack::EnchantmentId>);

#[derive(bevy_ecs::resource::Resource, Default)]
pub(crate) struct PendingConsumes(Vec<Entity>);

pub(crate) fn find_unbreaking_enchantment(
    mut found: system::ResMut<UnbreakingEnchantment>,
    registries: system::Res<mcrs_minecraft_registries::CurrentRegistries>,
) {
    let key = mcrs_minecraft_item::registry_keys::ENCHANTMENT.location();
    let name = mcrs_minecraft_item::enchantment_keys::UNBREAKING.location();
    let enchantment = registries.id_of(key, name).map(item_stack::EnchantmentId);
    found.set_if_neq(UnbreakingEnchantment(enchantment));
}

type PlayerRow = (
    &'static engine_world::session::Owner,
    &'static mcrs_minecraft_game_mode::PlayerGameMode,
    Option<&'static engine_item::Selected>,
    &'static engine_item::Slots,
    &'static entity_key::EntityUuid,
);

type StackRow = (
    Option<&'static engine_item::Removed>,
    Option<&'static item_stack::MaxDamage>,
    Option<&'static mut item_stack::Damage>,
    bevy_ecs::query::Has<item_stack::Unbreakable>,
    Option<&'static item_stack::Count>,
    Option<&'static item_stack::Enchantments>,
    Option<&'static item_stack::Tool>,
    Option<&'static item_stack::Weapon>,
    Option<&'static item_stack::Equippable>,
);

#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct DurabilityContext<'w, 's> {
    commands: system::Commands<'w, 's>,
    unbreaking: Option<system::Res<'w, UnbreakingEnchantment>>,
    clock: Option<system::Res<'w, engine_world::birth_clock::BirthClock>>,
    pending: system::ResMut<'w, PendingConsumes>,
    players: system::Query<'w, 's, PlayerRow>,
    stacks: system::Query<'w, 's, StackRow>,
    packets: message::MessageWriter<'w, bus::OutboundPlayerPacket>,
    signals: message::MessageWriter<'w, PlayerSignal>,
}

impl DurabilityContext<'_, '_> {
    fn slot_stack(&self, player: Entity, slot: DurabilitySlot) -> Option<Entity> {
        let (_, mode, selected, slots, _) = self.players.get(player).ok()?;
        if mode.0 == mcrs_minecraft_game_mode::GameMode::Creative {
            return None;
        }
        let slot_index = match slot {
            DurabilitySlot::MainHand => usize::from(selected.map_or(0, |entry| entry.0)),
            DurabilitySlot::OffHand => usize::from(mcrs_minecraft_inventory_player::OFF_HAND),
            DurabilitySlot::Armour(armour_slot) => usize::from(armour_slot.index()),
        };
        slots.get(slot_index)
    }

    fn apply_wear(&mut self, target: (Entity, DurabilitySlot, Entity), base: i32, salt: u64) {
        let (player, slot, stack) = target;
        let Ok((&owner, _, _, _, &uuid)) = self.players.get(player) else {
            return;
        };
        let Ok((removed, max_damage, Some(mut stored), unbreakable, count, enchantments, ..)) =
            self.stacks.get_mut(stack)
        else {
            return;
        };
        let mut used = 0_usize;
        for &entry in &self.pending.0 {
            used += usize::from(entry == stack);
        }
        let remaining = usize::from(count.copied().map_or(1, item_stack::Count::get));
        if unbreakable || used >= remaining {
            return;
        }
        let max_value = effective(max_damage, removed).map_or(0, |entry| entry.0);
        let current = effective(Some(&*stored), removed).map(|entry| entry.0);
        let (true, Some(current_damage)) = (max_value > 0, current) else {
            return;
        };
        let target_id = self.unbreaking.as_deref().and_then(|entry| entry.0);
        let entries = effective(enchantments, removed).map_or(&[][..], |entry| entry.entries());
        let found = target_id.and_then(|id| entries.iter().find(|entry| entry.enchantment == id));
        let level = found.map_or(0, |entry| i32::from(entry.level.get()));
        let seed = self.clock.as_deref().map_or(0, |clock| clock.0);
        let stream = entity_key::KeyedStream::new(&uuid, seed);
        let wear = durability::roll_unbreaking_salted(base, level, slot.wear_kind(), &stream, salt);
        if wear <= 0 {
            return;
        }
        let clamped = current_damage.clamp(0, max_value);
        let next = clamped.saturating_add(wear).min(max_value);
        let changed = stored.set_if_neq(item_stack::Damage(next));
        if next >= max_value {
            self.pending.0.push(stack);
            self.commands.entity(stack).consume(1);
            self.commands
                .queue(move |world: &mut bevy_ecs::world::World| {
                    if let Some(mut pending) = world.get_resource_mut::<PendingConsumes>()
                        && let Some(pos) = pending.0.iter().position(|&entry| entry == stack)
                    {
                        pending.0.swap_remove(pos);
                    }
                });
            let status = slot.break_event().status();
            let entity_id = mcrs_minecraft_engine::network_id::player_network_id(owner.0);
            let event = ClientboundEntityEvent {
                entity_id,
                entity_status: status,
            };
            self.packets
                .write(bus::to(owner.0, bus::PacketPayload::EntityEvent(event)));
            self.signals
                .write(PlayerSignal::new(player, MobSignal::EntityEvent { status }));
        } else if changed {
            self.commands.entity(stack).touch();
        }
    }

    fn wear_hand(&mut self, player: Entity, domain: u64, seq: &mut u32, hardness: Option<f32>) {
        let Some(stack) = self.slot_stack(player, DurabilitySlot::MainHand) else {
            return;
        };
        let Ok((removed, _, _, _, _, _, tool, weapon, _)) = self.stacks.get(stack) else {
            return;
        };
        let wear = match hardness {
            Some(value) => durability::block_break_wear(
                effective(tool, removed).map_or(0, |entry| entry.damage_per_block),
                value,
            ),
            None => durability::attack_wear(
                effective(weapon, removed).map_or(0, |entry| entry.item_damage_per_attack),
            ),
        };
        if wear > 0 {
            let slot = DurabilitySlot::MainHand;
            self.apply_wear((player, slot, stack), wear, domain | slot.salt(*seq));
            *seq = seq.wrapping_add(1);
        }
    }

    fn wear_armour(&mut self, target: Entity, raw_amount: f32, sequence: u32) {
        type Slot = ArmourSlot;
        use HurtWearPolicy::*;
        for armour_slot in [Slot::Head, Slot::Chest, Slot::Legs, Slot::Feet] {
            let slot = DurabilitySlot::Armour(armour_slot);
            let Some(stack) = self.slot_stack(target, slot) else {
                continue;
            };
            let Ok((removed, .., equippable)) = self.stacks.get(stack) else {
                continue;
            };
            let damages = effective(equippable, removed).is_some_and(|entry| entry.damage_on_hurt);
            let policy = if damages { DamagesOnHurt } else { IgnoresHurt };
            let wear = durability::armour_hit_wear(raw_amount, policy, AppliesToArmour);
            if wear > 0 {
                let salt = (3_u64 << 48) | slot.salt(sequence);
                self.apply_wear((target, slot, stack), wear, salt);
            }
        }
    }
}

pub(crate) fn on_block_broken(
    mut broken: message::MessageReader<engine_world::block_drops::BlockBroken>,
    blocks: Option<system::Res<mcrs_minecraft_block::definition::Blocks>>,
    mut context: DurabilityContext,
) {
    let mut sequence = 0_u32;
    for event in broken.read() {
        if let Some(blocks) = blocks.as_deref() {
            let hardness = blocks.state(event.state).hardness;
            if hardness.is_finite() && hardness > 0.0 {
                context.wear_hand(event.breaker, 1_u64 << 48, &mut sequence, Some(hardness));
            }
        }
    }
}

pub(crate) fn on_melee_attack(
    mut attacks: message::MessageReader<engine_damage::MeleeAttackHit>,
    mut context: DurabilityContext,
) {
    let mut sequence = 0_u32;
    for hit in attacks.read() {
        context.wear_hand(hit.attacker, 2_u64 << 48, &mut sequence, None);
    }
}

pub(crate) fn on_entity_damaged(
    mut damaged: message::MessageReader<engine_damage::Damaged>,
    rules: Option<system::Res<damage::DamageTypeRules>>,
    mut context: DurabilityContext,
) {
    let mut sequence = 0_u32;
    for hit in damaged.read() {
        if let Some(rules) = rules.as_deref()
            && hit.report.outcome != damage::HitOutcome::Rejected
            && !rules
                .tags(hit.source.damage_type)
                .contains(damage::DamageTags::BYPASSES_ARMOR)
        {
            context.wear_armour(hit.target, hit.report.raw_amount, sequence);
            sequence = sequence.wrapping_add(1);
        }
    }
}

pub(crate) fn on_extra_use(
    mut uses: message::MessageReader<crate::DurabilityExtraUse>,
    mut context: DurabilityContext,
) {
    let mut sequence = 0_u32;
    for extra in uses.read() {
        if extra.amount >= 1
            && let Some(stack) = context.slot_stack(extra.target, extra.slot)
        {
            let salt = (4_u64 << 48) | extra.slot.salt(sequence);
            context.apply_wear((extra.target, extra.slot, stack), extra.amount, salt);
            sequence = sequence.wrapping_add(1);
        }
    }
}
