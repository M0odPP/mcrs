use crate::lid_feedback::LidFeedback;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LidSoundTrigger {
    None,
    Open,
    Close,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarrelOpenState {
    Closed,
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LidTransition {
    pub viewers: u16,
    pub sound: LidSoundTrigger,
    pub block_event_param: Option<u8>,
    pub barrel_open: Option<BarrelOpenState>,
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ContainerLidState {
    viewers: u16,
}

const _: () = assert!(core::mem::size_of::<ContainerLidState>() == 2, "size 2");

fn transition(feedback: LidFeedback, viewers: u16, sound: LidSoundTrigger) -> LidTransition {
    use BarrelOpenState::{Closed, Open};
    let count = u8::try_from(viewers).unwrap_or(u8::MAX);
    let block_event_param = matches!(feedback, LidFeedback::BlockEvent).then_some(count);
    let barrel = if viewers == 0 { Closed } else { Open };
    let barrel_open = matches!(feedback, LidFeedback::OpenProperty).then_some(barrel);
    let mut resolved = sound;
    if matches!(feedback, LidFeedback::None) {
        resolved = LidSoundTrigger::None;
    }
    LidTransition {
        viewers,
        sound: resolved,
        block_event_param,
        barrel_open,
    }
}

impl ContainerLidState {
    #[must_use]
    pub const fn new() -> Self {
        Self { viewers: 0 }
    }

    #[must_use]
    pub const fn viewers(self) -> u16 {
        self.viewers
    }

    pub fn open(&mut self, feedback: LidFeedback) -> LidTransition {
        use LidSoundTrigger::{None, Open};
        let previous = self.viewers;
        self.viewers = self.viewers.saturating_add(1);
        let sound = if previous == 0 { Open } else { None };
        transition(feedback, self.viewers, sound)
    }

    pub fn close(&mut self, feedback: LidFeedback) -> LidTransition {
        use LidSoundTrigger::{Close, None};
        let previous = self.viewers;
        self.viewers = self.viewers.saturating_sub(1);
        if previous == 0 {
            return transition(LidFeedback::None, 0, None);
        }
        let sound = if previous == 1 { Close } else { None };
        transition(feedback, self.viewers, sound)
    }
}
