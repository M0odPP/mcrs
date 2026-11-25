#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i8)]
pub enum SlotBreakEvent {
    MainHand = 47,
    OffHand = 48,
    Feet = 49,
    Legs = 50,
    Chest = 51,
    Head = 52,
}

impl SlotBreakEvent {
    #[must_use]
    pub const fn status(self) -> i8 {
        self as i8
    }
}
