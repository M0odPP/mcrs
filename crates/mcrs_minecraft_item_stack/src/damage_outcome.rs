use crate::Kind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DamageOutcome {
    Unchanged,
    Damaged(i32),
    Broken(Kind),
}
