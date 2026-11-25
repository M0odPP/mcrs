#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeatherCycleRng(u64);

impl WeatherCycleRng {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_u32_bound(&mut self, bound: u32) -> u32 {
        if bound <= 1 {
            return 0;
        }
        ((u64::from(bound) * (self.next_u64() >> 32)) >> 32) as u32
    }

    pub fn inclusive_range(&mut self, min: u32, max: u32) -> u32 {
        min.saturating_add(self.next_u32_bound(max.saturating_sub(min).saturating_add(1)))
    }

    pub fn one_in(&mut self, denominator: u32) -> bool {
        denominator > 0 && self.next_u32_bound(denominator) == 0
    }
}
