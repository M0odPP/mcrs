use bevy_ecs::resource::Resource;

pub const DEFAULT_WORLD_BORDER_SIZE: f64 = 59_999_968.0;
pub const DEFAULT_WORLD_BORDER_SAFE_ZONE: f64 = 5.0;
pub const DEFAULT_WORLD_BORDER_DAMAGE_PER_BLOCK: f64 = 0.2;

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct DimensionWorldBorder {
    pub center_x: f64,
    pub center_z: f64,
    pub size: f64,
    pub damage_safe_zone: f64,
    pub damage_per_block: f64,
}

impl Default for DimensionWorldBorder {
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_z: 0.0,
            size: DEFAULT_WORLD_BORDER_SIZE,
            damage_safe_zone: DEFAULT_WORLD_BORDER_SAFE_ZONE,
            damage_per_block: DEFAULT_WORLD_BORDER_DAMAGE_PER_BLOCK,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldBorderBounds {
    pub min_x: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_z: f64,
}

const _: () = {
    assert!(
        size_of::<DimensionWorldBorder>() == 40,
        "DimensionWorldBorder size must match five f64 fields"
    );
    assert!(
        align_of::<DimensionWorldBorder>() == 8,
        "DimensionWorldBorder align must match f64 alignment"
    );
    assert!(
        size_of::<WorldBorderBounds>() == 32,
        "WorldBorderBounds size must match four f64 bounds"
    );
    assert!(
        align_of::<WorldBorderBounds>() == 8,
        "WorldBorderBounds align must match f64 alignment"
    );
};

impl DimensionWorldBorder {
    #[must_use]
    pub fn with_center_and_size(center_x: f64, center_z: f64, size: f64) -> Self {
        Self {
            center_x,
            center_z,
            size,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn bounds(&self) -> WorldBorderBounds {
        let half = self.size.max(0.0) * 0.5;
        WorldBorderBounds {
            min_x: self.center_x - half,
            min_z: self.center_z - half,
            max_x: self.center_x + half,
            max_z: self.center_z + half,
        }
    }
}
