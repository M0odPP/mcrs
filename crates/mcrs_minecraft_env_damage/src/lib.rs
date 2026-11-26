#![no_std]

macro_rules! check_layout {
    ($ty:ty, $size:expr, $align:expr, $name:literal) => {
        const _: () = {
            use core::mem::{align_of, size_of};
            assert!(size_of::<$ty>() == $size, concat!($name, " size"));
            assert!(align_of::<$ty>() == $align, concat!($name, " align"));
        };
    };
}

pub(crate) use check_layout;

mod air;
mod fall;
mod fire;
mod freeze;
mod hazard;

pub use air::*;
pub use fall::*;
pub use fire::*;
pub use freeze::*;
pub use hazard::*;
