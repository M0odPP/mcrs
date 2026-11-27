use core::mem::{align_of, size_of};

use crate::level_math::{
    death_drop_points, level_and_progress_for_total_xp, points_in_level, xp_needed_for_next_level,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExperienceUpdate {
    pub needs_sync: bool,
    pub leveled_up: bool,
}

impl ExperienceUpdate {
    #[must_use]
    pub const fn new(needs_sync: bool, leveled_up: bool) -> Self {
        Self {
            needs_sync,
            leveled_up,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct ExperienceState {
    pub level: i32,
    pub progress: f32,
    pub total: i32,
    pub score: i32,
    pub xp_seed: i32,
}

impl ExperienceState {
    #[must_use]
    pub const fn with_seed(xp_seed: i32) -> Self {
        Self {
            level: 0,
            progress: 0.0,
            total: 0,
            score: 0,
            xp_seed,
        }
    }

    #[must_use]
    pub fn from_total_points(total: i32, score: i32, xp_seed: i32) -> Self {
        let clamped = total.max(0);
        let mapped = level_and_progress_for_total_xp(clamped);
        Self {
            level: mapped.level,
            progress: mapped.progress,
            total: clamped,
            score,
            xp_seed,
        }
    }

    #[must_use]
    pub const fn needed_for_next_level(&self) -> i32 {
        xp_needed_for_next_level(self.level)
    }

    #[must_use]
    pub fn points_in_level(&self) -> i32 {
        points_in_level(self.level, self.progress)
    }

    pub fn add_points(&mut self, points: i32) -> ExperienceUpdate {
        if points == 0 {
            return ExperienceUpdate::new(true, false);
        }
        self.score = self.score.wrapping_add(points);
        let base = if self.progress.is_finite() {
            self.progress
        } else {
            0.0
        };
        self.progress = base + (points as f32) / (self.needed_for_next_level() as f32);
        let summed = (i64::from(self.total) + i64::from(points)).clamp(0, i64::from(i32::MAX));
        self.total = summed as i32;
        while self.progress < 0.0 {
            if !self.progress.is_finite() {
                self.progress = 0.0;
                break;
            }
            let remainder = self.progress * (self.needed_for_next_level() as f32);
            let before = self.level;
            let _ = self.add_levels(-1);
            let next = 1.0 + remainder / (self.needed_for_next_level() as f32);
            self.progress = if before > 0 { next } else { 0.0 };
        }
        let mut leveled_up = false;
        while self.progress >= 1.0 {
            if !self.progress.is_finite()
                || (self.level == i32::MAX && self.progress - 1.0 == self.progress)
            {
                self.progress = 0.0;
                break;
            }
            self.progress = (self.progress - 1.0) * (self.needed_for_next_level() as f32);
            let _ = self.add_levels(1);
            leveled_up = true;
            self.progress /= self.needed_for_next_level() as f32;
        }
        ExperienceUpdate::new(true, leveled_up)
    }

    pub fn add_levels(&mut self, levels: i32) -> ExperienceUpdate {
        self.level = self.level.saturating_add(levels);
        if self.level < 0 {
            self.level = 0;
            self.progress = 0.0;
            self.total = 0;
        }
        ExperienceUpdate::new(levels != 0, levels > 0)
    }

    pub fn spend_levels(&mut self, levels: i32) -> ExperienceUpdate {
        if levels <= 0 {
            ExperienceUpdate::default()
        } else {
            self.add_levels(-levels)
        }
    }

    #[must_use]
    pub const fn death_drop_points(&self) -> i32 {
        death_drop_points(self.level)
    }

    #[must_use]
    pub const fn reset_on_death(self) -> Self {
        Self::with_seed(self.xp_seed)
    }
}

const _: () = {
    let valid = size_of::<ExperienceUpdate>() == 2 && align_of::<ExperienceUpdate>() == 1;
    assert!(valid, "ExperienceUpdate layout");
    let valid = size_of::<ExperienceState>() == 20 && align_of::<ExperienceState>() == 4;
    assert!(valid, "ExperienceState layout");
};
