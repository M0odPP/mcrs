use alloc::string::String;
use alloc::vec::Vec;
use core::mem::{align_of, size_of};

const TICKS_PER_SECOND: f32 = 20.0;

#[must_use]
pub const fn cooldown_ticks(seconds: f32) -> u32 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    let scaled = seconds * TICKS_PER_SECOND;
    if scaled >= u32::MAX as f32 {
        u32::MAX
    } else {
        scaled as u32
    }
}

#[must_use]
pub const fn resolve_cooldown_group<'a>(
    cooldown_group: Option<&'a str>,
    item_id: &'a str,
) -> &'a str {
    match cooldown_group {
        Some(group) => group,
        None => item_id,
    }
}

#[must_use]
pub const fn remaining_ticks(end_tick: u64, current_tick: u64) -> u32 {
    let remaining = end_tick.saturating_sub(current_tick);
    if remaining >= u32::MAX as u64 {
        u32::MAX
    } else {
        remaining as u32
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CooldownEntry {
    pub group: String,
    pub start_tick: u64,
    pub end_tick: u64,
}

const _: () = assert!(
    size_of::<CooldownEntry>()
        == (16 + size_of::<String>()).next_multiple_of(align_of::<CooldownEntry>())
        && align_of::<CooldownEntry>() == align_of::<u64>(),
    "CooldownEntry packs group String and two u64 ticks"
);

impl CooldownEntry {
    #[must_use]
    pub const fn new(group: String, start_tick: u64, end_tick: u64) -> Self {
        Self {
            group,
            start_tick,
            end_tick,
        }
    }

    #[must_use]
    pub const fn remaining_ticks(&self, current_tick: u64) -> u32 {
        remaining_ticks(self.end_tick, current_tick)
    }

    #[must_use]
    pub const fn is_active(&self, current_tick: u64) -> bool {
        current_tick < self.end_tick
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct CooldownMap {
    entries: Vec<CooldownEntry>,
}

const _: () = assert!(
    size_of::<CooldownMap>() == size_of::<Vec<CooldownEntry>>()
        && align_of::<CooldownMap>() == align_of::<Vec<CooldownEntry>>(),
    "CooldownMap adds zero layout overhead beyond Vec<CooldownEntry>"
);

impl CooldownMap {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
        }
    }

    #[must_use]
    pub fn is_on_cooldown(&self, group: &str, current_tick: u64) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.group == group && entry.is_active(current_tick))
    }

    #[must_use]
    pub fn remaining_cooldown(&self, group: &str, current_tick: u64) -> u32 {
        self.entries
            .iter()
            .find(|entry| entry.group == group)
            .map_or(0, |entry| entry.remaining_ticks(current_tick))
    }

    pub fn start_cooldown(&mut self, group: &str, current_tick: u64, duration_ticks: u32) {
        if duration_ticks == 0 {
            let _ = self.remove_cooldown(group, current_tick);
            return;
        }
        let end_tick = current_tick.saturating_add(u64::from(duration_ticks));
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.group == group) {
            entry.start_tick = current_tick;
            entry.end_tick = end_tick;
        } else if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|entry| !entry.is_active(current_tick))
        {
            slot.group.clear();
            slot.group.push_str(group);
            slot.start_tick = current_tick;
            slot.end_tick = end_tick;
        } else {
            self.entries.push(CooldownEntry::new(
                String::from(group),
                current_tick,
                end_tick,
            ));
        }
        self.entries.retain(|entry| entry.is_active(current_tick));
    }

    pub fn remove_cooldown(&mut self, group: &str, current_tick: u64) -> bool {
        let mut removed_active = false;
        self.entries.retain(|entry| {
            if entry.group == group {
                removed_active = entry.is_active(current_tick);
                false
            } else {
                true
            }
        });
        removed_active
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn iter_active(&self, current_tick: u64) -> impl Iterator<Item = &CooldownEntry> {
        self.entries
            .iter()
            .filter(move |entry| entry.is_active(current_tick))
    }
}
