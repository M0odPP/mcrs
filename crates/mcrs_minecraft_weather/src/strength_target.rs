use crate::constants::STRENGTH_STEP;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrengthTarget {
    Decay,
    Rise,
}

impl StrengthTarget {
    #[must_use]
    pub(crate) const fn delta(self) -> f32 {
        match self {
            Self::Decay => -STRENGTH_STEP,
            Self::Rise => STRENGTH_STEP,
        }
    }
}
