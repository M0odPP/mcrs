use core::mem::align_of;
use core::mem::size_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DamageTypeTagId(pub u16);

const _: () = {
    assert!(
        size_of::<DamageTypeTagId>() == size_of::<u16>(),
        "DamageTypeTagId matches u16 size"
    );
    assert!(
        align_of::<DamageTypeTagId>() == align_of::<u16>(),
        "DamageTypeTagId matches u16 alignment"
    );
};
