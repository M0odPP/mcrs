use core::mem::{align_of, size_of};

use crate::constants::{MAX_PLAYER_DEATH_DROP_POINTS, PLAYER_DEATH_DROP_PER_LEVEL};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LevelProgress {
    pub level: i32,
    pub progress: f32,
}

#[must_use]
pub const fn xp_needed_for_next_level(level: i32) -> i32 {
    if level >= 30 {
        112_i32.saturating_add((level - 30).saturating_mul(9))
    } else if level >= 15 {
        5 * level - 38
    } else if level >= 0 {
        2 * level + 7
    } else {
        7
    }
}

#[must_use]
pub const fn total_xp_for_level(level: i32) -> i64 {
    if level <= 0 {
        return 0;
    }
    let wide = level as i128;
    let sum = if level <= 16 {
        wide * wide + 6 * wide
    } else if level <= 31 {
        (5 * wide * wide - 81 * wide + 720) / 2
    } else {
        (9 * wide * wide - 325 * wide + 4440) / 2
    };
    if sum >= i64::MAX as i128 {
        i64::MAX
    } else {
        sum as i64
    }
}

#[must_use]
pub fn points_in_level(level: i32, progress: f32) -> i32 {
    if progress.is_finite() {
        libm::floorf(progress * (xp_needed_for_next_level(level) as f32)) as i32
    } else {
        0
    }
}

#[must_use]
pub fn total_xp_at(level: i32, progress: f32) -> i64 {
    if level < 0 {
        0
    } else {
        total_xp_for_level(level).saturating_add(i64::from(points_in_level(level, progress).max(0)))
    }
}

#[must_use]
pub fn level_and_progress_for_total_xp(total: i32) -> LevelProgress {
    if total <= 0 {
        return LevelProgress::default();
    }
    let target = i64::from(total);
    let (mut low, mut high) = (0_i32, 21_863_i32);
    while low < high {
        let middle = low + (high - low + 1) / 2;
        if total_xp_for_level(middle) <= target {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let remainder = target.saturating_sub(total_xp_for_level(low));
    let needed = xp_needed_for_next_level(low) as f32;
    LevelProgress {
        level: low,
        progress: (remainder as f32) / needed,
    }
}

#[must_use]
pub const fn death_drop_points(level: i32) -> i32 {
    if level <= 0 {
        return 0;
    }
    let scaled = level.saturating_mul(PLAYER_DEATH_DROP_PER_LEVEL);
    if scaled > MAX_PLAYER_DEATH_DROP_POINTS {
        MAX_PLAYER_DEATH_DROP_POINTS
    } else {
        scaled
    }
}

const _: () = {
    let valid = size_of::<LevelProgress>() == 8 && align_of::<LevelProgress>() == 4;
    assert!(valid, "LevelProgress layout");
};
