#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LidFeedback {
    None,
    BlockEvent,
    OpenProperty,
}
