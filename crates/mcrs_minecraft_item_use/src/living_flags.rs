use crate::UseHand;

pub const LIVING_FLAG_USING_ITEM: u8 = 0x01;
pub const LIVING_FLAG_OFFHAND: u8 = 0x02;
pub const LIVING_FLAG_SPIN_ATTACK: u8 = 0x04;

#[must_use]
pub const fn living_flags_bits(previous: u8, active_hand: Option<UseHand>) -> u8 {
    match active_hand {
        Some(UseHand::Main) => (previous & !LIVING_FLAG_OFFHAND) | LIVING_FLAG_USING_ITEM,
        Some(UseHand::Off) => previous | LIVING_FLAG_USING_ITEM | LIVING_FLAG_OFFHAND,
        None => previous & !LIVING_FLAG_USING_ITEM,
    }
}
