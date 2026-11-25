use crate::constants::{LIGHTNING_ROD_RADIUS, RAIN_TEMPERATURE_MIN};

#[must_use]
pub fn within_rod_radius_sq(dx: i32, dy: i32, dz: i32) -> Option<u64> {
    let max_sq = u64::from(LIGHTNING_ROD_RADIUS.unsigned_abs()).pow(2);
    let dist_sq = u64::from(dx.unsigned_abs())
        .pow(2)
        .saturating_add(u64::from(dy.unsigned_abs()).pow(2))
        .saturating_add(u64::from(dz.unsigned_abs()).pow(2));
    (dist_sq <= max_sq).then_some(dist_sq)
}

#[must_use]
pub fn warm_enough_for_rain(base_temperature: f32) -> bool {
    base_temperature >= RAIN_TEMPERATURE_MIN
}
