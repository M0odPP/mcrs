pub const SUFFOCATION_EYE_BOX_SCALE: f32 = 0.8;
pub const SUFFOCATION_EYE_BOX_HEIGHT: f64 = 1.0e-6;
pub const CONTACT_BOX_SHRINK: f64 = 1.0e-5_f32 as f64;
pub const IN_WALL_DAMAGE: f32 = 1.0;
pub const CACTUS_DAMAGE: f32 = 1.0;
pub const CAMPFIRE_DAMAGE: f32 = 1.0;
pub const SOUL_CAMPFIRE_DAMAGE: f32 = 2.0;
pub const SWEET_BERRY_BUSH_DAMAGE: f32 = 1.0;
pub const SWEET_BERRY_MOVE_THRESHOLD: f64 = 0.003;
pub const HOT_FLOOR_DAMAGE: f32 = 1.0;
pub const VOID_DAMAGE: f32 = 4.0;
pub const VOID_MIN_Y_OFFSET: f64 = 64.0;
pub const DEFAULT_BORDER_SAFE_ZONE: f64 = 5.0;
pub const DEFAULT_BORDER_DAMAGE_PER_BLOCK: f64 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisAlignedBounds {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl AxisAlignedBounds {
    #[must_use]
    pub fn from_feet(feet_x: f64, feet_y: f64, feet_z: f64, width: f32, height: f32) -> Self {
        let half_width = f64::from(width) * 0.5;
        Self {
            min_x: feet_x - half_width,
            min_y: feet_y,
            min_z: feet_z - half_width,
            max_x: feet_x + half_width,
            max_y: feet_y + f64::from(height),
            max_z: feet_z + half_width,
        }
    }

    #[must_use]
    pub fn has_positive_volume(self) -> bool {
        self.max_x > self.min_x && self.max_y > self.min_y && self.max_z > self.min_z
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldBorderBounds {
    pub west: f64,
    pub north: f64,
    pub east: f64,
    pub south: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldBorderDamageConfig {
    pub safe_zone: f64,
    pub damage_per_block: f64,
}

impl Default for WorldBorderDamageConfig {
    fn default() -> Self {
        Self {
            safe_zone: DEFAULT_BORDER_SAFE_ZONE,
            damage_per_block: DEFAULT_BORDER_DAMAGE_PER_BLOCK,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuffocationState {
    Clear,
    InsideWall,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContactHazard {
    Cactus,
    CampfireLit,
    SoulCampfireLit,
    CampfireUnlit,
    SweetBerryBush { age: u8, delta_x: f64, delta_z: f64 },
    MagmaStanding,
    MagmaCrouching,
}

crate::check_layout!(AxisAlignedBounds, 48, 8, "AxisAlignedBounds");
crate::check_layout!(WorldBorderBounds, 32, 8, "WorldBorderBounds");
crate::check_layout!(WorldBorderDamageConfig, 16, 8, "WorldBorderDamageConfig");
crate::check_layout!(SuffocationState, 1, 1, "SuffocationState");
crate::check_layout!(ContactHazard, 24, 8, "ContactHazard");

#[must_use]
pub fn suffocation_eye_box(
    eye_x: f64,
    eye_y: f64,
    eye_z: f64,
    entity_width: f32,
) -> AxisAlignedBounds {
    let half_width = f64::from(SUFFOCATION_EYE_BOX_SCALE * entity_width) * 0.5;
    let half_height = SUFFOCATION_EYE_BOX_HEIGHT * 0.5;
    AxisAlignedBounds {
        min_x: eye_x - half_width,
        min_y: eye_y - half_height,
        min_z: eye_z - half_width,
        max_x: eye_x + half_width,
        max_y: eye_y + half_height,
        max_z: eye_z + half_width,
    }
}

#[must_use]
pub fn contact_damage(hazard: ContactHazard) -> Option<f32> {
    match hazard {
        ContactHazard::Cactus => Some(CACTUS_DAMAGE),
        ContactHazard::CampfireLit => Some(CAMPFIRE_DAMAGE),
        ContactHazard::SoulCampfireLit => Some(SOUL_CAMPFIRE_DAMAGE),
        ContactHazard::CampfireUnlit | ContactHazard::MagmaCrouching => None,
        ContactHazard::MagmaStanding => Some(HOT_FLOOR_DAMAGE),
        ContactHazard::SweetBerryBush {
            age,
            delta_x,
            delta_z,
        } => {
            let moved = libm::fabs(delta_x) >= SWEET_BERRY_MOVE_THRESHOLD
                || libm::fabs(delta_z) >= SWEET_BERRY_MOVE_THRESHOLD;
            (age != 0 && moved).then_some(SWEET_BERRY_BUSH_DAMAGE)
        }
    }
}

#[must_use]
pub fn void_damage(feet_y: f64, min_y: i32) -> Option<f32> {
    (feet_y < f64::from(min_y) - VOID_MIN_Y_OFFSET).then_some(VOID_DAMAGE)
}

const fn min_propagate_nan(left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() {
        f64::NAN
    } else if left < right {
        left
    } else {
        right
    }
}

#[must_use]
pub fn border_damage(
    feet_x: f64,
    feet_z: f64,
    body_bounds: AxisAlignedBounds,
    border: WorldBorderBounds,
    config: WorldBorderDamageConfig,
    suffocation: SuffocationState,
) -> Option<f32> {
    if suffocation == SuffocationState::InsideWall
        || config.damage_per_block.is_nan()
        || config.damage_per_block <= 0.0
    {
        return None;
    }
    let inside = body_bounds.min_x >= border.west
        && body_bounds.min_z >= border.north
        && body_bounds.max_x - CONTACT_BOX_SHRINK < border.east
        && body_bounds.max_z - CONTACT_BOX_SHRINK < border.south;
    if inside {
        return None;
    }
    let edge_x = min_propagate_nan(feet_x - border.west, border.east - feet_x);
    let edge_z = min_propagate_nan(feet_z - border.north, border.south - feet_z);
    let distance_to_edge = min_propagate_nan(edge_x, edge_z);
    let buffer = distance_to_edge + config.safe_zone;
    if buffer.is_nan() || buffer >= 0.0 {
        return None;
    }
    let scaled = libm::floor(-buffer * config.damage_per_block);
    let points = if scaled.is_nan() || scaled <= 1.0 {
        1
    } else if scaled >= i32::MAX as f64 {
        i32::MAX
    } else {
        scaled as i32
    };
    Some(points as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hazard_contact_void_and_border() {
        use ContactHazard::*;
        use SuffocationState::{Clear, InsideWall};
        let eye = suffocation_eye_box(0.5, 65.62, 0.5, 0.6_f32);
        let half_width = f64::from(0.8_f32 * 0.6_f32) * 0.5;
        let eye_ok = (eye.max_y - eye.min_y - 1.0e-6).abs() < 1.0e-12
            && eye.min_x == 0.5 - half_width
            && eye.max_x == 0.5 + half_width;
        assert!(eye_ok, "suffocation box uses 0.8w x 1e-6h");
        let berry = |age, delta_x, delta_z| SweetBerryBush {
            age,
            delta_x,
            delta_z,
        };
        let fire_ok = contact_damage(CampfireLit) == Some(1.0)
            && contact_damage(SoulCampfireLit) == Some(2.0)
            && contact_damage(CampfireUnlit).is_none();
        let hit_ok = contact_damage(Cactus) == Some(1.0)
            && fire_ok
            && contact_damage(MagmaStanding) == Some(1.0)
            && contact_damage(MagmaCrouching).is_none()
            && void_damage(-128.0, -64).is_none()
            && void_damage(-129.0, -64) == Some(4.0);
        assert!(hit_ok, "cactus, campfire, magma, void damage");
        let berry_ok = contact_damage(berry(0, 0.5, 0.0)).is_none()
            && contact_damage(berry(1, 0.0029, 0.0029)).is_none()
            && contact_damage(berry(1, 0.003, 0.0)) == Some(1.0);
        assert!(berry_ok, "sweet berry needs age and >=0.003 move");
        let border = WorldBorderBounds {
            west: -9.5,
            north: -9.5,
            east: 10.5,
            south: 10.5,
        };
        let config = WorldBorderDamageConfig::default();
        let check = |position_x, position_z, suffocation| {
            let bounds = AxisAlignedBounds::from_feet(position_x, 64.0, position_z, 0.6, 1.8);
            border_damage(position_x, position_z, bounds, border, config, suffocation)
        };
        let border_ok = check(0.5, 0.5, Clear).is_none()
            && check(15.0, 0.5, Clear).is_none()
            && check(16.0, 0.5, Clear) == Some(1.0)
            && check(25.5, 0.5, Clear) == Some(2.0)
            && check(25.5, 0.5, InsideWall).is_none()
            && check(25.5, f64::NAN, Clear).is_none()
            && check(f64::INFINITY, 0.5, Clear) == Some(i32::MAX as f32);
        assert!(border_ok, "border checks buffer, wall, NaN, infinity");
    }
}
