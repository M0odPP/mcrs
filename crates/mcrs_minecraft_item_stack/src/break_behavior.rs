#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BreakBehavior {
    BreakAtMax,
    ClampBelowMax,
}
