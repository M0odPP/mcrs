use crate::weather_events::WeatherEvents;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeatherLevelStep {
    pub rain: f32,
    pub thunder: f32,
    pub events: WeatherEvents,
}
