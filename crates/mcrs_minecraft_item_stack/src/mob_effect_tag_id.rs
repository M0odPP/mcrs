use core::mem::{align_of, size_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MobEffectTagId(pub u16);

const _: () = {
    assert!(
        size_of::<MobEffectTagId>() == 2 && align_of::<MobEffectTagId>() == 2,
        "MobEffectTagId wraps one u16"
    );
};
