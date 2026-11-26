use core::mem::align_of;
use core::mem::size_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DamageTypeId(pub u16);

const _: () = {
    assert!(
        size_of::<DamageTypeId>() == size_of::<u16>(),
        "DamageTypeId matches u16 size"
    );
    assert!(
        align_of::<DamageTypeId>() == align_of::<u16>(),
        "DamageTypeId matches u16 alignment"
    );
};
