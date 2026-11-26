use alloc::boxed::Box;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::mem::{align_of, size_of};

use crate::EffectDetails;
use crate::MobEffectId;
use crate::effect_details::EffectFields;

#[non_exhaustive]
pub struct EffectInstance {
    pub effect: MobEffectId,
    pub amplifier: u8,
    pub duration: i32,
    pub ambient: bool,
    pub show_particles: bool,
    pub show_icon: bool,
    pub hidden_effect: Option<Box<EffectDetails>>,
}

const _: () = {
    assert!(
        size_of::<EffectInstance>()
            == (12 + size_of::<usize>()).next_multiple_of(align_of::<EffectInstance>()),
        "EffectInstance packs effect id, duration, flags, and optional boxed child"
    );
    assert!(
        align_of::<EffectInstance>() == align_of::<EffectDetails>(),
        "EffectInstance matches EffectDetails alignment"
    );
};

impl EffectInstance {
    #[must_use]
    pub const fn new(effect: MobEffectId) -> Self {
        Self {
            effect,
            amplifier: 0,
            duration: 0,
            ambient: false,
            show_particles: true,
            show_icon: true,
            hidden_effect: None,
        }
    }

    #[must_use]
    pub(crate) fn fields(&self) -> EffectFields<'_> {
        EffectFields {
            amplifier: self.amplifier,
            duration: self.duration,
            ambient: self.ambient,
            show_particles: self.show_particles,
            show_icon: self.show_icon,
            hidden_effect: self.hidden_effect.as_deref(),
        }
    }
}

impl Drop for EffectInstance {
    fn drop(&mut self) {
        EffectDetails::drop_chain(&mut self.hidden_effect);
    }
}

impl Clone for EffectInstance {
    fn clone(&self) -> Self {
        Self {
            effect: self.effect,
            amplifier: self.amplifier,
            duration: self.duration,
            ambient: self.ambient,
            show_particles: self.show_particles,
            show_icon: self.show_icon,
            hidden_effect: EffectDetails::clone_chain(self.hidden_effect.as_deref()),
        }
    }
}

impl PartialEq for EffectInstance {
    fn eq(&self, other: &Self) -> bool {
        self.effect == other.effect
            && self.amplifier == other.amplifier
            && self.duration == other.duration
            && self.ambient == other.ambient
            && self.show_particles == other.show_particles
            && self.show_icon == other.show_icon
            && EffectDetails::eq_chain(
                self.hidden_effect.as_deref(),
                other.hidden_effect.as_deref(),
            )
    }
}

impl Eq for EffectInstance {}

impl Hash for EffectInstance {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.effect.hash(state);
        self.amplifier.hash(state);
        self.duration.hash(state);
        self.ambient.hash(state);
        self.show_particles.hash(state);
        self.show_icon.hash(state);
        EffectDetails::hash_chain(self.hidden_effect.as_deref(), state);
    }
}

impl fmt::Debug for EffectInstance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EffectInstance")
            .field("effect", &self.effect)
            .field("amplifier", &self.amplifier)
            .field("duration", &self.duration)
            .field("ambient", &self.ambient)
            .field("show_particles", &self.show_particles)
            .field("show_icon", &self.show_icon)
            .field(
                "hidden_effect",
                &EffectDetails::debug_hidden(self.hidden_effect.as_deref()),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use core::hash::{Hash, Hasher};

    use alloc::boxed::Box;
    use alloc::format;

    use crate::{EffectDetails, EffectInstance, MobEffectId};

    struct ByteMixHasher(u64);

    impl Hasher for ByteMixHasher {
        fn finish(&self) -> u64 {
            self.0
        }

        fn write(&mut self, bytes: &[u8]) {
            for &byte in bytes {
                self.0 = self.0.wrapping_mul(131).wrapping_add(u64::from(byte));
            }
        }
    }

    #[test]
    fn deep_hidden_effect_chain_is_iterative_across_clone_eq_hash_debug_and_drop() {
        const CHAIN_DEPTH: usize = 50_000;
        let mut head: Option<Box<EffectDetails>> = None;
        for step in 0..CHAIN_DEPTH {
            let mut node = EffectDetails::new();
            node.duration = step as i32;
            node.hidden_effect = head;
            head = Some(Box::new(node));
        }
        let mut instance = EffectInstance::new(MobEffectId(1));
        instance.hidden_effect = head;

        let cloned = instance.clone();
        assert_eq!(
            instance, cloned,
            "deep EffectInstance chain clone must equal original"
        );

        let mut mutated = cloned.clone();
        if let Some(child) = mutated.hidden_effect.as_deref_mut() {
            child.amplifier = child.amplifier.wrapping_add(1);
        }
        assert_ne!(
            instance, mutated,
            "modifying deep EffectInstance chain must compare unequal"
        );

        let mut hash_left = ByteMixHasher(0);
        let mut hash_right = ByteMixHasher(0);
        instance.hash(&mut hash_left);
        cloned.hash(&mut hash_right);
        assert_eq!(
            hash_left.finish(),
            hash_right.finish(),
            "equal deep EffectInstance chains must hash identically"
        );

        let formatted = format!("{instance:?}");
        assert!(
            formatted.contains("Some(..)"),
            "deep Debug output must bound recursion with Some(..)"
        );

        drop(mutated);
        drop(cloned);
        drop(instance);
    }
}
