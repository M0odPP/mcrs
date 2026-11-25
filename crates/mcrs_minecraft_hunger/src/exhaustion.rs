pub const MAX_EXHAUSTION: f32 = 40.0;
pub const EXHAUSTION_DRAIN_THRESHOLD: f32 = 4.0;
pub const ATTACK_EXHAUSTION: f32 = 0.1;
pub const HARVEST_BREAK_EXHAUSTION: f32 = 0.005;
pub const JUMP_EXHAUSTION: f32 = 0.05;
pub const SPRINT_JUMP_EXHAUSTION: f32 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SprintState {
    Idle,
    Sprinting,
}

#[must_use]
pub const fn jump_exhaustion(sprint: SprintState) -> f32 {
    match sprint {
        SprintState::Sprinting => SPRINT_JUMP_EXHAUSTION,
        SprintState::Idle => JUMP_EXHAUSTION,
    }
}

#[must_use]
pub fn sprint_move_exhaustion(horizontal_distance: f32) -> f32 {
    if !horizontal_distance.is_finite() || horizontal_distance <= 0.0 {
        return 0.0;
    }
    let cm = libm::roundf(horizontal_distance * 100.0);
    if !cm.is_finite() || cm <= 0.0 {
        return 0.0;
    }
    0.1 * cm * 0.01
}

#[must_use]
pub fn hunger_effect_exhaustion(amplifier: u8) -> f32 {
    0.005 * (f32::from(amplifier) + 1.0)
}
