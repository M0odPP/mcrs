use alloc::vec::Vec;

use crate::BreakBehavior;
use crate::Builtin;
use crate::BuiltinKey;
use crate::Count;
use crate::Damage;
use crate::DamageOutcome;
use crate::ItemComponent;
use crate::ItemDefaults;
use crate::Kind;
use crate::MaxDamage;
use crate::PatchEntry;
use crate::patch_key::PatchKey;

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct ItemStack {
    pub kind: Kind,
    pub count: Count,
    patch: Vec<PatchEntry>,
}

impl ItemStack {
    #[must_use]
    pub fn new(kind: Kind, count: Count) -> Self {
        Self {
            kind,
            count,
            patch: Vec::new(),
        }
    }

    #[must_use]
    pub fn set<T: ItemComponent>(mut self, value: T) -> Self {
        self.put(T::patch_key(), Some(value.into_entry()));
        self
    }

    #[must_use]
    pub fn unset<T: ItemComponent>(mut self) -> Self {
        self.put(T::patch_key(), T::builtin_key().map(PatchEntry::Removal));
        self
    }

    #[must_use]
    pub fn patch(&self) -> &[PatchEntry] {
        &self.patch
    }

    #[must_use]
    pub fn mergeable(&self, other: &Self) -> bool {
        self.kind == other.kind && self.patch == other.patch
    }

    #[doc(hidden)]
    #[must_use]
    pub fn with_entry(mut self, entry: PatchEntry) -> Self {
        self.put_entry(entry);
        self
    }

    #[must_use]
    pub fn effective<'a, T: ItemComponent + 'static>(
        &'a self,
        defaults: &'a impl ItemDefaults,
    ) -> Option<&'a T> {
        let key = T::patch_key();
        if let Some(entry) = self.patch_entry(key) {
            return match entry {
                PatchEntry::Value(value) => T::from_builtin(value),
                _ => None,
            };
        }
        if let Some(found) = defaults
            .default_components(self.kind)
            .iter()
            .find_map(T::from_builtin)
        {
            return Some(found);
        }
        let _ = T::builtin_key()?;
        T::neutral()
    }

    #[must_use]
    pub fn has_effective(&self, defaults: &impl ItemDefaults, key: BuiltinKey) -> bool {
        if let Some(entry) = self.patch_entry(PatchKey::Builtin(key)) {
            return !matches!(
                entry,
                PatchEntry::Removal(_) | PatchEntry::UndecodedRemoval(_)
            );
        }
        defaults
            .default_components(self.kind)
            .iter()
            .any(|component| component.key() == key)
    }

    #[must_use]
    pub fn is_damageable(&self, defaults: &impl ItemDefaults) -> bool {
        self.has_effective(defaults, BuiltinKey::MaxDamage)
            && self.has_effective(defaults, BuiltinKey::Damage)
            && !self.has_effective(defaults, BuiltinKey::Unbreakable)
    }

    #[must_use]
    pub fn max_damage(&self, defaults: &impl ItemDefaults) -> i32 {
        self.effective::<MaxDamage>(defaults).map_or(0, |max| max.0)
    }

    #[must_use]
    pub fn damage_value(&self, defaults: &impl ItemDefaults) -> i32 {
        let damage = self
            .effective::<Damage>(defaults)
            .map_or(0, |damage| damage.0);
        damage.min(self.max_damage(defaults)).max(0)
    }

    pub fn set_damage(&mut self, damage: i32, defaults: &impl ItemDefaults) {
        let clamped = damage.min(self.max_damage(defaults)).max(0);
        self.set_normalized(Builtin::Damage(Damage(clamped)), defaults);
    }

    #[must_use]
    pub fn is_damaged(&self, defaults: &impl ItemDefaults) -> bool {
        self.is_damageable(defaults) && self.damage_value(defaults) > 0
    }

    #[must_use]
    pub fn is_broken(&self, defaults: &impl ItemDefaults) -> bool {
        self.is_damageable(defaults) && self.damage_value(defaults) >= self.max_damage(defaults)
    }

    #[must_use]
    pub fn next_damage_will_break(&self, defaults: &impl ItemDefaults) -> bool {
        self.is_damageable(defaults)
            && self.damage_value(defaults) >= self.max_damage(defaults).saturating_sub(1)
    }

    #[must_use]
    pub fn apply_damage(
        mut self,
        amount: i32,
        behavior: BreakBehavior,
        defaults: &impl ItemDefaults,
    ) -> (Option<Self>, DamageOutcome) {
        if !self.is_damageable(defaults) || amount == 0 {
            return (Some(self), DamageOutcome::Unchanged);
        }
        let current = self.damage_value(defaults);
        let max = self.max_damage(defaults);
        let sum = current.saturating_add(amount);
        match behavior {
            BreakBehavior::ClampBelowMax => {
                let ceiling = max.saturating_sub(1).max(0);
                let target = sum.clamp(0, ceiling);
                self.set_damage(target, defaults);
                let next = self.damage_value(defaults);
                let outcome = if next == current {
                    DamageOutcome::Unchanged
                } else {
                    DamageOutcome::Damaged(next)
                };
                (Some(self), outcome)
            }
            BreakBehavior::BreakAtMax => {
                self.set_damage(sum, defaults);
                if self.is_broken(defaults) {
                    let kind = self.kind;
                    let remaining = self.count.get().saturating_sub(1);
                    let next_stack = Count::new(remaining).map(|count| {
                        self.count = count;
                        self
                    });
                    (next_stack, DamageOutcome::Broken(kind))
                } else {
                    let next = self.damage_value(defaults);
                    let outcome = if next == current {
                        DamageOutcome::Unchanged
                    } else {
                        DamageOutcome::Damaged(next)
                    };
                    (Some(self), outcome)
                }
            }
        }
    }

    fn patch_entry(&self, key: PatchKey<'_>) -> Option<&PatchEntry> {
        let index = self
            .patch
            .binary_search_by(|present| present.key().cmp(&key))
            .ok()?;
        self.patch.get(index)
    }

    fn set_normalized(&mut self, value: Builtin, defaults: &impl ItemDefaults) {
        let key = PatchKey::Builtin(value.key());
        let matches_default = defaults
            .default_components(self.kind)
            .iter()
            .any(|default| default == &value);
        if matches_default {
            self.put(key, None);
        } else {
            self.put(key, Some(PatchEntry::Value(value)));
        }
    }

    pub(crate) fn put_entry(&mut self, entry: PatchEntry) {
        let found = self
            .patch
            .binary_search_by(|present| present.key().cmp(&entry.key()));
        match found {
            Ok(index) => {
                if let Some(present) = self.patch.get_mut(index) {
                    *present = entry;
                }
            }
            Err(index) => self.patch.insert(index, entry),
        }
    }

    fn put(&mut self, key: PatchKey<'static>, entry: Option<PatchEntry>) {
        match entry {
            Some(entry) => self.put_entry(entry),
            None => {
                if let Ok(index) = self
                    .patch
                    .binary_search_by(|present| present.key().cmp(&key))
                {
                    self.patch.remove(index);
                }
            }
        }
    }
}
