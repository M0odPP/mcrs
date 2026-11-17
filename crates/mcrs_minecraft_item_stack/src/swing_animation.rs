use mcrs_minecraft_swing::{SwingHand, SwingKind, SwingSpec};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SwingAnimation {
    pub kind: SwingKind,
    pub duration: i32,
}

impl SwingAnimation {
    pub const DEFAULT_KIND: SwingKind = SwingKind::Whack;
    pub const DEFAULT_DURATION: i32 = 6;
    pub const DEFAULT: Self = Self {
        kind: Self::DEFAULT_KIND,
        duration: Self::DEFAULT_DURATION,
    };

    #[must_use]
    pub fn spec_or_default(hand: SwingHand, animation: Option<Self>) -> SwingSpec {
        match animation {
            Some(anim) => SwingSpec::new(hand, anim.kind, anim.duration.max(0)),
            None => SwingSpec {
                hand,
                ..SwingSpec::DEFAULT_MAIN
            },
        }
    }
}

impl Default for SwingAnimation {
    fn default() -> Self {
        Self::DEFAULT
    }
}
