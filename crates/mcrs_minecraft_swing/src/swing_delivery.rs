#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SwingDelivery {
    #[default]
    ObserversOnly,
    SelfAndObservers,
}
