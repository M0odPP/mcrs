use crate::{SwingHand, SwingKind};

pub const DEFAULT_DURATION: i32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SwingSpec {
    pub hand: SwingHand,
    pub kind: SwingKind,
    pub duration: i32,
}

impl SwingSpec {
    pub const DEFAULT_MAIN: Self = Self {
        hand: SwingHand::Main,
        kind: SwingKind::Whack,
        duration: DEFAULT_DURATION,
    };

    #[must_use]
    pub const fn new(hand: SwingHand, kind: SwingKind, duration: i32) -> Self {
        Self {
            hand,
            kind,
            duration,
        }
    }
}

impl Default for SwingSpec {
    fn default() -> Self {
        Self::DEFAULT_MAIN
    }
}
