use std::mem::{align_of, size_of};

use bevy_ecs::change_detection::{DetectChangesMut, Mut};
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{Message, MessageReader, MessageWriter};
use bevy_ecs::query::{Has, With};
use bevy_ecs::system::{Commands, Populated, Query, SystemParam};
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload, to};
use mcrs_minecraft_engine::loaded_client::UnloadedClient;
use mcrs_minecraft_engine::network_id::player_network_id;
use mcrs_minecraft_engine::player_sprinting::PlayerSprinting;
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine::{LivingFlags, UsingItem};
use mcrs_minecraft_engine_item::{Selected, Slots, StackCommandsExt, Stacks};
use mcrs_minecraft_engine_world::session::Owner;
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_inventory_player::{HOTBAR, OFF_HAND};
use mcrs_minecraft_item_stack::{BlocksAttacks, Consumable, Food, Kind, UseEffects};
use mcrs_minecraft_item_use::{
    SprintPolicy, UseHand, UseKind, UseOutcome, UseSpec, cancel_on_slot_change, release_use,
    start_use, tick_use,
};
use mcrs_minecraft_player_holders::PlayerHolders;
use mcrs_minecraft_protocol_game::Hand;
use mcrs_minecraft_protocol_game::clientbound::ClientboundEntityEvent;
use smallvec::SmallVec;

use crate::{CarriedSlotChanged, ReleaseUseItem, StartUseItem};

const MAX_FOOD_LEVEL: i32 = 20;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub(crate) struct UsingTarget {
    stack: Entity,
    kind: Kind,
}

#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FinishUseItem {
    player: Entity,
    stack: Entity,
    hand: UseHand,
    hotbar_slot: u8,
    kind: Kind,
}

const _: () = {
    assert!(
        size_of::<UsingTarget>() == 16 && align_of::<UsingTarget>() == 8,
        "UsingTarget stores Entity and Kind within 16 bytes"
    );
    assert!(
        size_of::<FinishUseItem>() == 24 && align_of::<FinishUseItem>() == 8,
        "FinishUseItem stores player, stack, hand, slot, and kind within 24 bytes"
    );
};

type InputPlayerRow<'a> = (
    Option<&'a UsingItem>,
    &'a mut LivingFlags,
    Option<&'a mut PlayerSprinting>,
    &'a Slots,
    &'a Selected,
    Option<&'a PlayerGameMode>,
    Option<&'a Vitals>,
    Has<UnloadedClient>,
    &'a PlayerFileState,
);

type TickPlayerRow<'a> = (
    Entity,
    &'a mut UsingItem,
    &'a UsingTarget,
    &'a mut LivingFlags,
    Option<&'a mut PlayerSprinting>,
    &'a Slots,
    &'a Selected,
    Option<&'a PlayerGameMode>,
    Option<&'a Vitals>,
    Has<UnloadedClient>,
);

type FinishPlayerRow<'a> = (
    &'a Owner,
    &'a Slots,
    &'a Selected,
    Option<&'a Vitals>,
    Option<&'a PlayerGameMode>,
    Has<UnloadedClient>,
);

#[derive(SystemParam)]
pub(crate) struct UseInputMessages<'w, 's> {
    starts: MessageReader<'w, 's, StartUseItem>,
    releases: MessageReader<'w, 's, ReleaseUseItem>,
    slot_changes: MessageReader<'w, 's, CarriedSlotChanged>,
}

#[derive(SystemParam)]
pub(crate) struct FinishContext<'w, 's> {
    players: Query<'w, 's, FinishPlayerRow<'static>, With<PlayerHolders>>,
    stacks: Stacks<'w, 's>,
    packets: MessageWriter<'w, OutboundPlayerPacket>,
    commands: Commands<'w, 's>,
}

pub(crate) fn handle_use_item_inputs(
    mut inputs: UseInputMessages<'_, '_>,
    mut players: Query<InputPlayerRow<'_>, With<PlayerHolders>>,
    mut stacks: Stacks<'_, '_>,
    mut finishes: MessageWriter<FinishUseItem>,
    mut commands: Commands,
) {
    let released: SmallVec<[Entity; 4]> = inputs
        .releases
        .read()
        .map(|message| message.player)
        .collect();
    let changed_slots: SmallVec<[(Entity, u8); 4]> = inputs
        .slot_changes
        .read()
        .map(|message| (message.player, message.slot))
        .collect();

    let mut stopped: SmallVec<[Entity; 4]> = SmallVec::new();
    for &player in &released {
        if !stopped.contains(&player)
            && let Ok((Some(active), flags, ..)) = players.get_mut(player)
            && release_use(Some(active.0)).is_some()
        {
            stopped.push(player);
            clear_active_use(player, flags, commands.reborrow());
        }
    }
    for &(player, slot) in &changed_slots {
        if !stopped.contains(&player)
            && let Ok((Some(active), flags, ..)) = players.get_mut(player)
            && cancel_on_slot_change(active.0, slot).is_none()
        {
            stopped.push(player);
            clear_active_use(player, flags, commands.reborrow());
        }
    }

    let mut started: SmallVec<[Entity; 4]> = SmallVec::new();
    for &start in inputs.starts.read() {
        if started.contains(&start.player) {
            continue;
        }
        let Ok((
            active,
            mut flags,
            sprinting,
            slots,
            selected,
            game_mode,
            vitals,
            unloaded,
            file_state,
        )) = players.get_mut(start.player)
        else {
            continue;
        };
        let active_state = if stopped.contains(&start.player) {
            None
        } else {
            active.map(|component| component.0)
        };
        if active_state.is_some() || !can_act(unloaded, vitals, game_mode) {
            continue;
        }
        let Some((use_hand, hotbar_slot, slot_index)) = hand_slot(start.hand, selected.0) else {
            continue;
        };
        let Some(stack) = slots.get(usize::from(slot_index)) else {
            continue;
        };
        let Some(kind) = stacks.kind(stack) else {
            continue;
        };
        let Some(spec) = classify_use_spec(&mut stacks, stack, is_creative(game_mode), file_state)
        else {
            continue;
        };
        started.push(start.player);
        let Some(state) = start_use(None, use_hand, hotbar_slot, spec) else {
            continue;
        };
        let cancelled_same_tick = released.contains(&start.player)
            || changed_slots.iter().any(|&(player, slot)| {
                player == start.player && cancel_on_slot_change(state, slot).is_none()
            });
        if cancelled_same_tick {
            continue;
        }
        if spec.kind == UseKind::Consumable && spec.use_duration_ticks == 0 {
            finishes.write(FinishUseItem {
                player: start.player,
                stack,
                hand: use_hand,
                hotbar_slot,
                kind,
            });
            continue;
        }
        clear_sprint_if_disallowed(spec.sprint_policy, sprinting);
        flags.set_if_neq(flags.with_active_hand(Some(use_hand)));
        commands
            .entity(start.player)
            .insert((UsingItem(state), UsingTarget { stack, kind }));
    }
}

pub(crate) fn tick_using_items(
    mut players: Populated<TickPlayerRow<'_>, With<PlayerHolders>>,
    stacks: Stacks<'_, '_>,
    mut finishes: MessageWriter<FinishUseItem>,
    mut commands: Commands,
) {
    for (
        player,
        mut using,
        target,
        flags,
        sprinting,
        slots,
        selected,
        game_mode,
        vitals,
        unloaded,
    ) in &mut players
    {
        let slot_index = active_slot_index(using.0.hand, using.0.slot);
        let valid = can_act(unloaded, vitals, game_mode)
            && cancel_on_slot_change(using.0, selected.0).is_some()
            && slots.get(usize::from(slot_index)) == Some(target.stack)
            && stacks.kind(target.stack) == Some(target.kind);
        if !valid {
            clear_active_use(player, flags, commands.reborrow());
            continue;
        }
        let next_state = match tick_use(using.0) {
            UseOutcome::Continuing { state, .. } => Some(state),
            UseOutcome::Completed { .. } => None,
        };
        if let Some(state) = next_state {
            clear_sprint_if_disallowed(state.sprint_policy, sprinting);
            using.set_if_neq(UsingItem(state));
        } else {
            clear_active_use(player, flags, commands.reborrow());
            finishes.write(FinishUseItem {
                player,
                stack: target.stack,
                hand: using.0.hand,
                hotbar_slot: using.0.slot,
                kind: target.kind,
            });
        }
    }
}

pub(crate) fn finish_use_items(
    mut finishes: MessageReader<FinishUseItem>,
    mut context: FinishContext<'_, '_>,
) {
    for &finish in finishes.read() {
        complete_use(finish, &mut context);
    }
}

fn complete_use(finish: FinishUseItem, context: &mut FinishContext<'_, '_>) {
    if context.stacks.kind(finish.stack) != Some(finish.kind) {
        return;
    }
    let Ok((owner, slots, selected, vitals, game_mode, unloaded)) =
        context.players.get_mut(finish.player)
    else {
        return;
    };
    let slot_index = active_slot_index(finish.hand, finish.hotbar_slot);
    let still_held = can_act(unloaded, vitals, game_mode)
        && match finish.hand {
            UseHand::Main => selected.0 == finish.hotbar_slot,
            UseHand::Off => true,
        }
        && slots.get(usize::from(slot_index)) == Some(finish.stack);
    if !still_held {
        return;
    }

    context.packets.write(to(
        owner.0,
        PacketPayload::EntityEvent(ClientboundEntityEvent {
            entity_id: player_network_id(owner.0),
            entity_status: ClientboundEntityEvent::USE_ITEM_FINISH,
        }),
    ));
    if !is_creative(game_mode) {
        context.commands.entity(finish.stack).consume(1);
    }
}

fn classify_use_spec(
    stacks: &mut Stacks<'_, '_>,
    stack: Entity,
    creative: bool,
    file_state: &PlayerFileState,
) -> Option<UseSpec> {
    let use_effects = stacks.get::<UseEffects>(stack).copied().unwrap_or_default();
    let sprint_policy = if use_effects.can_sprint {
        SprintPolicy::Allow
    } else {
        SprintPolicy::Forbid
    };
    if let Some(consumable) = stacks.get::<Consumable>(stack) {
        let consume_seconds = consumable.consume_seconds;
        let food = stacks.get::<Food>(stack);
        if !creative
            && food.is_some_and(|food| !food.can_always_eat)
            && file_state.food_level >= MAX_FOOD_LEVEL
        {
            return None;
        }
        return Some(UseSpec::consumable(
            consume_seconds,
            use_effects.speed_multiplier,
            sprint_policy,
        ));
    }
    stacks
        .get::<BlocksAttacks>(stack)
        .is_some()
        .then(|| UseSpec::continuous(use_effects.speed_multiplier, sprint_policy))
}

fn can_act(unloaded: bool, vitals: Option<&Vitals>, game_mode: Option<&PlayerGameMode>) -> bool {
    !unloaded
        && vitals.is_none_or(|health| health.is_alive() && health.health > 0.0)
        && game_mode.is_none_or(|mode| mode.0 != GameMode::Spectator)
}

fn is_creative(game_mode: Option<&PlayerGameMode>) -> bool {
    game_mode.is_some_and(|mode| mode.0 == GameMode::Creative)
}

fn clear_sprint_if_disallowed(
    sprint_policy: SprintPolicy,
    sprinting: Option<Mut<PlayerSprinting>>,
) {
    if sprint_policy == SprintPolicy::Forbid
        && let Some(mut sprint) = sprinting
    {
        sprint.set_if_neq(PlayerSprinting(false));
    }
}

fn hand_slot(hand: Hand, selected: u8) -> Option<(UseHand, u8, u16)> {
    match hand {
        Hand::Main if HOTBAR.contains(&u16::from(selected)) => {
            Some((UseHand::Main, selected, u16::from(selected)))
        }
        Hand::Main => None,
        Hand::Off => Some((UseHand::Off, 0, OFF_HAND)),
    }
}

fn active_slot_index(hand: UseHand, hotbar_slot: u8) -> u16 {
    match hand {
        UseHand::Main => u16::from(hotbar_slot),
        UseHand::Off => OFF_HAND,
    }
}

fn clear_active_use(player: Entity, mut flags: Mut<LivingFlags>, mut commands: Commands) {
    flags.set_if_neq(flags.with_active_hand(None));
    commands.entity(player).remove::<(UsingItem, UsingTarget)>();
}
