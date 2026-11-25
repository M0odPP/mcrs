#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum WeatherGameEvent {
    #[default]
    BeginRaining,
    EndRaining,
    RainLevelChange(f32),
    ThunderLevelChange(f32),
}
