use crate::HungerDifficulty;

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct HungerTickInput {
    pub health: f32,
    pub max_health: f32,
    pub difficulty: HungerDifficulty,
    pub natural_regeneration: bool,
    pub invulnerable: bool,
    pub lived_ticks: u64,
}

impl HungerTickInput {
    #[must_use]
    pub const fn new(
        health: f32,
        max_health: f32,
        difficulty: HungerDifficulty,
        natural_regeneration: bool,
        invulnerable: bool,
        lived_ticks: u64,
    ) -> Self {
        Self {
            health,
            max_health,
            difficulty,
            natural_regeneration,
            invulnerable,
            lived_ticks,
        }
    }
}
