#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WeatherState {
    pub clear_ticks: u32,
    pub rain_ticks: u32,
    pub thunder_ticks: u32,
    pub raining: bool,
    pub thundering: bool,
}
