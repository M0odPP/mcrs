use crate::Builtin;
use crate::Kind;

pub trait ItemDefaults {
    fn default_components(&self, kind: Kind) -> &[Builtin];
}

impl ItemDefaults for () {
    fn default_components(&self, _kind: Kind) -> &[Builtin] {
        &[]
    }
}
