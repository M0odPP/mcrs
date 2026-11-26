#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UseAnimation {
    None,
    #[default]
    Eat,
    Drink,
    Block,
    Bow,
    Trident,
    Crossbow,
    Spyglass,
    TootHorn,
    Brush,
    Bundle,
    Spear,
}

impl UseAnimation {
    pub(crate) const ALL: [Self; 12] = [
        Self::None,
        Self::Eat,
        Self::Drink,
        Self::Block,
        Self::Bow,
        Self::Trident,
        Self::Crossbow,
        Self::Spyglass,
        Self::TootHorn,
        Self::Brush,
        Self::Bundle,
        Self::Spear,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Eat => "eat",
            Self::Drink => "drink",
            Self::Block => "block",
            Self::Bow => "bow",
            Self::Trident => "trident",
            Self::Crossbow => "crossbow",
            Self::Spyglass => "spyglass",
            Self::TootHorn => "toot_horn",
            Self::Brush => "brush",
            Self::Bundle => "bundle",
            Self::Spear => "spear",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.name() == name)
    }
}

const _: () = {
    assert!(
        core::mem::size_of::<UseAnimation>() == 1 && core::mem::align_of::<UseAnimation>() == 1,
        "UseAnimation is a single-byte enum"
    );
};
