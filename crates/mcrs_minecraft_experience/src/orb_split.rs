use core::iter::FusedIterator;
use core::mem::{align_of, size_of};

use crate::constants::ORB_VALUE_TIERS;

#[must_use]
pub const fn largest_orb_value(amount: i32) -> i32 {
    if amount <= 0 {
        return 0;
    }
    let mut rest: &[i32] = &ORB_VALUE_TIERS;
    while let [tier, tail @ ..] = rest {
        if amount >= *tier {
            return *tier;
        }
        rest = tail;
    }
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct OrbValueSplit {
    remaining: i32,
}

#[must_use = "iterate yielded orb values"]
pub const fn split_orb_values(amount: i32) -> OrbValueSplit {
    OrbValueSplit {
        remaining: if amount > 0 { amount } else { 0 },
    }
}

impl Iterator for OrbValueSplit {
    type Item = i32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining <= 0 {
            return None;
        }
        let tier = largest_orb_value(self.remaining);
        self.remaining -= tier;
        Some(tier)
    }
}

impl FusedIterator for OrbValueSplit {}

const _: () = {
    let valid = size_of::<OrbValueSplit>() == 4 && align_of::<OrbValueSplit>() == 4;
    assert!(valid, "OrbValueSplit layout");
};
