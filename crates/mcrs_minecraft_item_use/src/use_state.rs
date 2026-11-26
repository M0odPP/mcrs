use core::mem::{align_of, size_of};

use crate::{SprintPolicy, UseHand, UseKind, UseSpec, should_emit_consume_tick_effects};

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct ItemUseState {
    pub elapsed_ticks: u32,
    pub use_duration_ticks: u32,
    pub speed_multiplier: f32,
    pub hand: UseHand,
    pub slot: u8,
    pub sprint_policy: SprintPolicy,
    pub kind: UseKind,
}

const _: () = assert!(
    size_of::<ItemUseState>() == 16,
    "ItemUseState packs three 32-bit fields and four 1-byte fields into 16 bytes"
);
const _: () = assert!(
    align_of::<ItemUseState>() == 4,
    "ItemUseState stays 4-byte aligned"
);

impl ItemUseState {
    #[must_use]
    pub const fn new(hand: UseHand, slot: u8, spec: UseSpec) -> Self {
        Self {
            elapsed_ticks: 0,
            use_duration_ticks: spec.use_duration_ticks,
            speed_multiplier: spec.speed_multiplier,
            hand,
            slot,
            sprint_policy: spec.sprint_policy,
            kind: spec.kind,
        }
    }

    #[must_use]
    pub const fn remaining_ticks(self) -> u32 {
        self.use_duration_ticks.saturating_sub(self.elapsed_ticks)
    }

    #[must_use]
    pub const fn permits_sprint(self) -> bool {
        matches!(self.sprint_policy, SprintPolicy::Allow)
    }

    #[must_use]
    pub const fn is_continuous(self) -> bool {
        matches!(self.kind, UseKind::Continuous)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UseOutcome {
    Continuing {
        state: ItemUseState,
        tick_effects: bool,
    },
    Completed {
        state: ItemUseState,
        tick_effects: bool,
    },
}

const _: () = assert!(
    size_of::<UseOutcome>() == 20 && align_of::<UseOutcome>() == 4,
    "UseOutcome stores state plus two flags with 4-byte alignment"
);

#[must_use]
pub const fn start_use(
    active: Option<ItemUseState>,
    hand: UseHand,
    slot: u8,
    spec: UseSpec,
) -> Option<ItemUseState> {
    if active.is_some() {
        return None;
    }
    Some(ItemUseState::new(hand, slot, spec))
}

#[must_use]
pub const fn tick_use(mut state: ItemUseState) -> UseOutcome {
    state.elapsed_ticks = state.elapsed_ticks.saturating_add(1);
    if state.is_continuous() {
        return UseOutcome::Continuing {
            state,
            tick_effects: false,
        };
    }
    let remaining = state.remaining_ticks();
    let tick_effects = should_emit_consume_tick_effects(state.use_duration_ticks, remaining);
    if state.elapsed_ticks >= state.use_duration_ticks {
        UseOutcome::Completed {
            state,
            tick_effects,
        }
    } else {
        UseOutcome::Continuing {
            state,
            tick_effects,
        }
    }
}

#[must_use]
pub const fn release_use(active: Option<ItemUseState>) -> Option<ItemUseState> {
    active
}

#[must_use]
pub const fn cancel_on_slot_change(state: ItemUseState, new_slot: u8) -> Option<ItemUseState> {
    if matches!(state.hand, UseHand::Main) && state.slot != new_slot {
        None
    } else {
        Some(state)
    }
}
