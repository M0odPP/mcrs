#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct HungerTickOutput {
    pub next_health: Option<f32>,
    pub starve: bool,
}

impl HungerTickOutput {
    #[must_use]
    pub const fn new(next_health: Option<f32>, starve: bool) -> Self {
        Self {
            next_health,
            starve,
        }
    }
}
