#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContainerMenuKind {
    Generic9x3,
    Generic9x6,
    Generic3x3,
    Hopper,
    ShulkerBox,
}

impl ContainerMenuKind {
    const fn spec(self) -> (&'static str, u8) {
        match self {
            Self::Generic9x3 => ("minecraft:generic_9x3", 27),
            Self::Generic9x6 => ("minecraft:generic_9x6", 54),
            Self::Generic3x3 => ("minecraft:generic_3x3", 9),
            Self::Hopper => ("minecraft:hopper", 5),
            Self::ShulkerBox => ("minecraft:shulker_box", 27),
        }
    }

    #[must_use]
    pub const fn key(self) -> &'static str {
        self.spec().0
    }

    #[must_use]
    pub const fn slot_count(self) -> u8 {
        self.spec().1
    }

    #[must_use]
    pub const fn total_menu_slots(self) -> u8 {
        self.slot_count().saturating_add(36)
    }
}
