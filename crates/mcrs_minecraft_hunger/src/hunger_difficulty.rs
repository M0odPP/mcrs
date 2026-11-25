#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HungerDifficulty {
    Peaceful,
    Easy,
    #[default]
    Normal,
    Hard,
}
