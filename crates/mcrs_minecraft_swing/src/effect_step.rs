#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectStep {
    Active,
    Expired,
    Inactive,
}

impl EffectStep {
    pub fn advance(duration: &mut i32) -> Self {
        if *duration == -1 {
            return Self::Active;
        }
        if *duration <= 0 {
            return Self::Inactive;
        }
        *duration -= 1;
        if *duration > 0 {
            Self::Active
        } else {
            Self::Expired
        }
    }
}
