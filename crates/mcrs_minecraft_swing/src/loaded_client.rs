#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct LoadedClient {
    loaded: bool,
    ticks_since_spawn: u8,
}

impl LoadedClient {
    pub const FALLBACK_TICKS: u8 = 60;

    #[must_use]
    pub const fn is_loaded(self) -> bool {
        self.loaded || self.ticks_since_spawn >= Self::FALLBACK_TICKS
    }

    pub fn mark_loaded(&mut self) {
        self.loaded = true;
    }

    pub fn reset(&mut self) {
        self.loaded = false;
        self.ticks_since_spawn = 0;
    }

    pub fn tick(&mut self) {
        if !self.loaded {
            self.ticks_since_spawn = self.ticks_since_spawn.saturating_add(1);
            if self.ticks_since_spawn >= Self::FALLBACK_TICKS {
                self.loaded = true;
            }
        }
    }
}
