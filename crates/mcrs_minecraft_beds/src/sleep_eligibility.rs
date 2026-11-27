use core::mem::{align_of, size_of};
use mcrs_voxel::BlockPos;

use crate::bed_geometry::{BedPair, WorldPoint};
use crate::bed_rule::{BedRuleCondition, BedRuleSpec};

pub const BED_REACH_HORIZONTAL: f64 = 3.0;
pub const BED_REACH_VERTICAL: f64 = 2.0;
pub const MONSTER_CHECK_HALF_EXTENT_XZ: f64 = 8.0;
pub const MONSTER_CHECK_HALF_EXTENT_Y: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityAabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl EntityAabb {
    #[must_use]
    pub fn strictly_intersects(self, other: Self) -> bool {
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
            && self.min_z < other.max_z
            && self.max_z > other.min_z
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObstructionState {
    Clear,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreativeBypass {
    Standard,
    Creative,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SleepRejection {
    Occupied,
    Obstructed,
    TooFarAway,
    CannotSleepNow,
    RuleDisallowed,
    NotSafe,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BedUseOutcome {
    Explode,
    SleepStarted,
    Rejected {
        reason: SleepRejection,
        sets_spawn: bool,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SleepCheckInput {
    pub rule: BedRuleSpec,
    pub occupied: bool,
    pub within_reach: bool,
    pub obstruction: ObstructionState,
    pub sky_light_level: f32,
    pub monster_nearby: bool,
    pub creative_bypass: CreativeBypass,
}

impl SleepCheckInput {
    #[must_use]
    pub const fn new(rule: BedRuleSpec, sky_light_level: f32) -> Self {
        Self {
            rule,
            occupied: false,
            within_reach: true,
            obstruction: ObstructionState::Clear,
            sky_light_level,
            monster_nearby: false,
            creative_bypass: CreativeBypass::Standard,
        }
    }
}

const fn reject_without_spawn(reason: SleepRejection) -> BedUseOutcome {
    BedUseOutcome::Rejected {
        reason,
        sets_spawn: false,
    }
}

#[must_use]
pub fn is_within_bed_reach(player_pos: WorldPoint, bed_pos: BlockPos) -> bool {
    let dx = libm::fabs(player_pos.x - (f64::from(bed_pos.x) + 0.5));
    let dy = libm::fabs(player_pos.y - (f64::from(bed_pos.y) + 0.5));
    let dz = libm::fabs(player_pos.z - (f64::from(bed_pos.z) + 0.5));
    dx <= BED_REACH_HORIZONTAL && dy <= BED_REACH_VERTICAL && dz <= BED_REACH_HORIZONTAL
}

#[must_use]
pub fn bed_in_reach(player_pos: WorldPoint, pair: BedPair) -> bool {
    is_within_bed_reach(player_pos, pair.head) || is_within_bed_reach(player_pos, pair.foot)
}

#[must_use]
pub fn monster_search_bounds(head_pos: BlockPos) -> EntityAabb {
    let center_x = f64::from(head_pos.x) + 0.5;
    let bottom_y = f64::from(head_pos.y);
    let center_z = f64::from(head_pos.z) + 0.5;
    let horizontal_limit = MONSTER_CHECK_HALF_EXTENT_XZ;
    let vertical_limit = MONSTER_CHECK_HALF_EXTENT_Y;
    EntityAabb {
        min_x: center_x - horizontal_limit,
        min_y: bottom_y - vertical_limit,
        min_z: center_z - horizontal_limit,
        max_x: center_x + horizontal_limit,
        max_y: bottom_y + vertical_limit,
        max_z: center_z + horizontal_limit,
    }
}

#[must_use]
pub fn entity_blocks_sleep(head_pos: BlockPos, monster_box: EntityAabb) -> bool {
    monster_search_bounds(head_pos).strictly_intersects(monster_box)
}

#[must_use]
pub fn evaluate_bed_use(input: &SleepCheckInput) -> BedUseOutcome {
    if input.rule.destroy_on_use {
        return BedUseOutcome::Explode;
    }
    if input.occupied {
        return reject_without_spawn(SleepRejection::Occupied);
    }
    let sets_spawn =
        crate::bed_rule::can_condition_pass(input.rule.can_set_spawn, input.sky_light_level);
    if !crate::bed_rule::can_condition_pass(input.rule.can_sleep, input.sky_light_level) {
        let reason = match input.rule.can_sleep {
            BedRuleCondition::Never => SleepRejection::RuleDisallowed,
            BedRuleCondition::Always | BedRuleCondition::WhenDark => SleepRejection::CannotSleepNow,
        };
        return BedUseOutcome::Rejected { reason, sets_spawn };
    }
    if !input.within_reach {
        return reject_without_spawn(SleepRejection::TooFarAway);
    }
    if input.obstruction == ObstructionState::Blocked {
        return reject_without_spawn(SleepRejection::Obstructed);
    }
    if input.monster_nearby && input.creative_bypass == CreativeBypass::Standard {
        let reason = SleepRejection::NotSafe;
        return BedUseOutcome::Rejected { reason, sets_spawn };
    }
    BedUseOutcome::SleepStarted
}

const _: () = {
    const fn layout<T>(size: usize, align: usize) -> bool {
        size_of::<T>() == size && align_of::<T>() == align
    }
    assert!(layout::<EntityAabb>(48, 8), "EntityAabb layout");
    assert!(layout::<SleepCheckInput>(16, 4), "SleepCheckInput layout");
};
