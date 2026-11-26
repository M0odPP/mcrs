use alloc::borrow::Cow;
use core::mem::align_of;
use core::mem::size_of;

use crate::BlockSet;
use crate::DamageTypeId;
use crate::DamageTypeTagId;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DamageTypeSet {
    Tag(DamageTypeTagId),
    Types(Cow<'static, [DamageTypeId]>),
}

const _: () = {
    assert!(
        size_of::<DamageTypeSet>() == size_of::<BlockSet>(),
        "DamageTypeSet matches BlockSet size"
    );
    assert!(
        align_of::<DamageTypeSet>() == align_of::<BlockSet>(),
        "DamageTypeSet matches BlockSet alignment"
    );
};
