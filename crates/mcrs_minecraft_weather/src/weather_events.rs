use crate::weather_game_event::WeatherGameEvent;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeatherEvents {
    events: [WeatherGameEvent; 5],
    len: u8,
}

impl WeatherEvents {
    #[must_use]
    pub fn as_slice(&self) -> &[WeatherGameEvent] {
        self.events.get(..usize::from(self.len)).unwrap_or(&[])
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub(crate) fn push(&mut self, event: WeatherGameEvent) {
        if let Some(slot) = self.events.get_mut(usize::from(self.len)) {
            *slot = event;
            self.len += 1;
        }
    }
}
