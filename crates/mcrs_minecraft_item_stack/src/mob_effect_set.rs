use alloc::borrow::Cow;
use core::mem::{align_of, size_of};

use crate::BlockSet;
use crate::MobEffectId;
use crate::MobEffectTagId;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MobEffectSet {
    Tag(MobEffectTagId),
    Effects(Cow<'static, [MobEffectId]>),
}

const _: () = {
    assert!(
        size_of::<MobEffectSet>() == size_of::<BlockSet>(),
        "MobEffectSet matches BlockSet size"
    );
    assert!(
        align_of::<MobEffectSet>() == align_of::<BlockSet>(),
        "MobEffectSet matches BlockSet alignment"
    );
};
