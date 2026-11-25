use core::mem::{align_of, size_of};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EquipSlot {
    MainHand = 0,
    OffHand = 1,
    Feet = 2,
    Legs = 3,
    Chest = 4,
    Head = 5,
}

const _: () = assert!(
    size_of::<EquipSlot>() == 1,
    "EquipSlot must occupy one byte"
);
const _: () = assert!(
    align_of::<EquipSlot>() == 1,
    "EquipSlot must have one-byte alignment"
);

impl EquipSlot {
    pub const COUNT: usize = 6;

    pub const ALL: [Self; Self::COUNT] = [
        Self::MainHand,
        Self::OffHand,
        Self::Feet,
        Self::Legs,
        Self::Chest,
        Self::Head,
    ];

    pub const ARMOUR: [Self; 4] = [Self::Feet, Self::Legs, Self::Chest, Self::Head];

    #[must_use]
    pub const fn wire_id(self) -> u8 {
        self as u8
    }

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn is_armour(self) -> bool {
        matches!(self, Self::Feet | Self::Legs | Self::Chest | Self::Head)
    }

    #[must_use]
    pub const fn from_wire_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::MainHand),
            1 => Some(Self::OffHand),
            2 => Some(Self::Feet),
            3 => Some(Self::Legs),
            4 => Some(Self::Chest),
            5 => Some(Self::Head),
            _ => None,
        }
    }

    #[must_use]
    pub const fn player_slot(self, selected_hotbar: u8) -> u16 {
        match self {
            Self::MainHand if (selected_hotbar as u16) < 9 => selected_hotbar as u16,
            Self::MainHand => 0,
            Self::OffHand => 40,
            Self::Feet => 36,
            Self::Legs => 37,
            Self::Chest => 38,
            Self::Head => 39,
        }
    }

    #[must_use]
    pub const fn from_player_slot(slot: u16, selected_hotbar: u8) -> Option<Self> {
        let hand = if (selected_hotbar as u16) < 9 {
            selected_hotbar as u16
        } else {
            0
        };
        if slot == hand {
            return Some(Self::MainHand);
        }
        match slot {
            40 => Some(Self::OffHand),
            36 => Some(Self::Feet),
            37 => Some(Self::Legs),
            38 => Some(Self::Chest),
            39 => Some(Self::Head),
            _ => None,
        }
    }
}
