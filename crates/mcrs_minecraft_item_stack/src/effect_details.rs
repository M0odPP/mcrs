use alloc::boxed::Box;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::mem::{align_of, size_of};

const MAX_DEBUG_DEPTH: usize = 16;

#[non_exhaustive]
pub struct EffectDetails {
    pub amplifier: u8,
    pub duration: i32,
    pub ambient: bool,
    pub show_particles: bool,
    pub show_icon: bool,
    pub hidden_effect: Option<Box<EffectDetails>>,
}

const _: () = {
    assert!(
        size_of::<EffectDetails>() == 8 + size_of::<usize>(),
        "EffectDetails packs duration, flags, and optional boxed child"
    );
    assert!(
        align_of::<EffectDetails>()
            == if align_of::<usize>() > 4 {
                align_of::<usize>()
            } else {
                4
            },
        "EffectDetails aligns to pointer or i32 alignment"
    );
};

impl EffectDetails {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            amplifier: 0,
            duration: 0,
            ambient: false,
            show_particles: true,
            show_icon: true,
            hidden_effect: None,
        }
    }

    pub(crate) fn drop_chain(head: &mut Option<Box<EffectDetails>>) {
        let mut next = head.take();
        while let Some(mut node) = next {
            next = node.hidden_effect.take();
        }
    }

    pub(crate) fn clone_chain(head: Option<&EffectDetails>) -> Option<Box<EffectDetails>> {
        let first = head?;
        let mut root = Box::new(first.clone_shallow());
        let mut tail = &mut root.hidden_effect;
        let mut cursor = first.hidden_effect.as_deref();
        while let Some(node) = cursor {
            let inserted = tail.insert(Box::new(node.clone_shallow()));
            tail = &mut inserted.hidden_effect;
            cursor = node.hidden_effect.as_deref();
        }
        Some(root)
    }

    pub(crate) fn eq_chain(
        mut left: Option<&EffectDetails>,
        mut right: Option<&EffectDetails>,
    ) -> bool {
        while let (Some(left_node), Some(right_node)) = (left, right) {
            if !left_node.shallow_eq(right_node) {
                return false;
            }
            left = left_node.hidden_effect.as_deref();
            right = right_node.hidden_effect.as_deref();
        }
        left.is_none() && right.is_none()
    }

    pub(crate) fn hash_chain<H: Hasher>(mut cursor: Option<&EffectDetails>, state: &mut H) {
        while let Some(node) = cursor {
            true.hash(state);
            node.hash_shallow(state);
            cursor = node.hidden_effect.as_deref();
        }
        false.hash(state);
    }

    fn clone_shallow(&self) -> Self {
        Self {
            amplifier: self.amplifier,
            duration: self.duration,
            ambient: self.ambient,
            show_particles: self.show_particles,
            show_icon: self.show_icon,
            hidden_effect: None,
        }
    }

    fn shallow_eq(&self, other: &Self) -> bool {
        self.amplifier == other.amplifier
            && self.duration == other.duration
            && self.ambient == other.ambient
            && self.show_particles == other.show_particles
            && self.show_icon == other.show_icon
    }

    fn hash_shallow<H: Hasher>(&self, state: &mut H) {
        self.amplifier.hash(state);
        self.duration.hash(state);
        self.ambient.hash(state);
        self.show_particles.hash(state);
        self.show_icon.hash(state);
    }

    pub(crate) fn debug_hidden(head: Option<&EffectDetails>) -> impl fmt::Debug + '_ {
        HiddenEffectDebug(head)
    }

    fn fmt_shallow_head(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "EffectDetails {{ amplifier: {:?}, duration: {:?}, ambient: {:?}, show_particles: {:?}, show_icon: {:?}, hidden_effect: ",
            self.amplifier, self.duration, self.ambient, self.show_particles, self.show_icon
        )
    }
}

impl Default for EffectDetails {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EffectDetails {
    fn drop(&mut self) {
        Self::drop_chain(&mut self.hidden_effect);
    }
}

impl Clone for EffectDetails {
    fn clone(&self) -> Self {
        let mut cloned = self.clone_shallow();
        cloned.hidden_effect = Self::clone_chain(self.hidden_effect.as_deref());
        cloned
    }
}

impl PartialEq for EffectDetails {
    fn eq(&self, other: &Self) -> bool {
        Self::eq_chain(Some(self), Some(other))
    }
}

impl Eq for EffectDetails {}

impl Hash for EffectDetails {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash_shallow(state);
        Self::hash_chain(self.hidden_effect.as_deref(), state);
    }
}

impl fmt::Debug for EffectDetails {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_shallow_head(formatter)?;
        fmt::Debug::fmt(&HiddenEffectDebug(self.hidden_effect.as_deref()), formatter)?;
        formatter.write_str(" }")
    }
}

struct HiddenEffectDebug<'node>(Option<&'node EffectDetails>);

impl fmt::Debug for HiddenEffectDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(first) = self.0 else {
            return formatter.write_str("None");
        };
        let mut opened = 0usize;
        let mut cursor = Some(first);
        while let Some(node) = cursor {
            if opened >= MAX_DEBUG_DEPTH {
                formatter.write_str("Some(..)")?;
                break;
            }
            formatter.write_str("Some(")?;
            opened += 1;
            node.fmt_shallow_head(formatter)?;
            cursor = node.hidden_effect.as_deref();
            if cursor.is_none() {
                formatter.write_str("None")?;
            }
        }
        for _ in 0..opened {
            formatter.write_str(" })")?;
        }
        Ok(())
    }
}
