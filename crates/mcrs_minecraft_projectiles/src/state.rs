use core::mem::{align_of, size_of};

use glam::DVec3;
use mcrs_minecraft_entity_key::EntityId;
use mcrs_voxel::{BlockPos, BlockStateId};

pub(crate) fn finite_f32(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

pub(crate) fn finite_dvec3(value: DVec3) -> DVec3 {
    if value.is_finite() {
        value
    } else {
        DVec3::ZERO
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectileAabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl ProjectileAabb {
    #[must_use]
    pub const fn new(min: DVec3, max: DVec3) -> Self {
        Self { min, max }
    }

    #[must_use]
    pub fn of_body(position: DVec3, width: f32, height: f32) -> Self {
        let half = f64::from(width * 0.5);
        let span = DVec3::new(half, 0.0, half);
        let top = DVec3::new(half, f64::from(height), half);
        Self::new(position - span, position + top)
    }

    #[must_use]
    pub fn inflate(self, amount: f64) -> Self {
        let padding = DVec3::splat(amount);
        Self::new(self.min - padding, self.max + padding)
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.min.is_finite() && self.max.is_finite() && self.min.cmple(self.max).all()
    }

    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.is_valid()
            && other.is_valid()
            && (self.min.cmplt(other.max) & self.max.cmpgt(other.min)).all()
    }

    #[must_use]
    pub fn clip_segment(&self, start: DVec3, end: DVec3) -> Option<f64> {
        let delta = end - start;
        if !self.is_valid() || !start.is_finite() || !end.is_finite() || !delta.is_finite() {
            return None;
        }
        let (mut near, mut far) = (0.0_f64, 1.0_f64);
        let mut slab = |origin: f64, step: f64, low: f64, high: f64| {
            if step == 0.0 {
                return origin >= low && origin <= high;
            }
            let (first, second) = ((low - origin) / step, (high - origin) / step);
            (near, far) = (near.max(first.min(second)), far.min(first.max(second)));
            near <= far
        };
        let hit = slab(start.x, delta.x, self.min.x, self.max.x)
            && slab(start.y, delta.y, self.min.y, self.max.y)
            && slab(start.z, delta.z, self.min.z, self.max.z);
        hit.then_some(near)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProjectileMedium {
    #[default]
    Air,
    Water,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct EntityTarget {
    pub id: EntityId,
    pub bounds: ProjectileAabb,
    pub position: DVec3,
    pub aimable: bool,
    pub in_owner_group: bool,
    pub arrow_harm_allowed: bool,
}

impl EntityTarget {
    #[must_use]
    pub const fn new(id: EntityId, bounds: ProjectileAabb, position: DVec3) -> Self {
        Self {
            id,
            bounds,
            position,
            aimable: true,
            in_owner_group: false,
            arrow_harm_allowed: true,
        }
    }
}

pub trait ProjectileWorld {
    fn block_state(&self, _position: BlockPos) -> BlockStateId {
        BlockStateId::AIR
    }

    fn visit_block_boxes(&self, _position: BlockPos, _visit: impl FnMut(ProjectileAabb) -> bool) {}

    fn medium_at(&self, _position: DVec3, _bounds: ProjectileAabb) -> ProjectileMedium {
        ProjectileMedium::Air
    }
}

pub trait ProjectileRandom {
    fn next_f32(&mut self) -> f32;
    fn next_f64(&mut self) -> f64;
    fn next_i32_bound(&mut self, bound: i32) -> i32;
}

#[cfg(feature = "random")]
impl<R: mcrs_minecraft_random::Random> ProjectileRandom for R {
    fn next_f32(&mut self) -> f32 {
        R::next_f32(self)
    }

    fn next_f64(&mut self) -> f64 {
        R::next_f64(self)
    }

    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        if bound <= 1 {
            return 0;
        }
        R::next_i32_bound(self, bound)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlightCategory {
    Arrow,
    Thrown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProjectileKind {
    #[default]
    Arrow,
    SpectralArrow,
    Snowball,
    Egg,
    EnderPearl,
    SplashPotion,
    LingeringPotion,
    Trident,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct ProjectileSpec {
    pub gravity: f64,
    pub air_drag: f32,
    pub water_drag: f32,
    pub width: f32,
    pub height: f32,
    pub base_damage: f64,
    pub category: FlightCategory,
}

impl ProjectileKind {
    #[must_use]
    pub const fn spec(self) -> ProjectileSpec {
        use FlightCategory::{Arrow as ArrowFlight, Thrown as ThrownFlight};
        let (category, gravity, water_drag, size, base_damage) = match self {
            Self::Arrow | Self::SpectralArrow => (ArrowFlight, 0.05, 0.6, 0.5, 2.0),
            Self::Trident => (ArrowFlight, 0.05, 0.99, 0.5, 8.0),
            Self::Snowball | Self::Egg | Self::EnderPearl => (ThrownFlight, 0.03, 0.8, 0.25, 0.0),
            Self::SplashPotion | Self::LingeringPotion => (ThrownFlight, 0.05, 0.8, 0.25, 0.0),
        };
        ProjectileSpec {
            gravity,
            air_drag: 0.99,
            water_drag,
            width: size,
            height: size,
            base_damage,
            category,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PickupPolicy {
    #[default]
    Disallowed = 0,
    Allowed = 1,
    CreativeOnly = 2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::component::Component))]
pub struct ProjectileState {
    pub kind: ProjectileKind,
    pub position: DVec3,
    pub velocity: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub owner: Option<EntityId>,
    pub left_owner: bool,
    pub age_ticks: u32,
    pub no_gravity: bool,
    pub in_water: bool,
    pub removed: bool,
    pub base_damage: f64,
    pub critical: bool,
    pub pierce_level: u8,
    pub pierced: [Option<EntityId>; 16],
    pub pierced_count: u8,
    pub pickup: PickupPolicy,
    pub in_ground: bool,
    pub shake_ticks: u8,
    pub despawn_ticks: u16,
    pub in_block_position: Option<BlockPos>,
    pub in_block_state: Option<BlockStateId>,
    pub loyalty_level: u8,
    pub returning: bool,
}

impl ProjectileState {
    #[must_use]
    pub fn new(
        kind: ProjectileKind,
        position: DVec3,
        velocity: DVec3,
        owner: Option<EntityId>,
    ) -> Self {
        let mut state = Self::default();
        (state.kind, state.position, state.velocity) = (kind, position, velocity);
        (state.owner, state.left_owner) = (owner, owner.is_none());
        state.base_damage = kind.spec().base_damage;
        state
    }

    #[must_use]
    pub fn bounds(&self) -> ProjectileAabb {
        let spec = self.kind.spec();
        ProjectileAabb::of_body(self.position, spec.width, spec.height)
    }
}

pub(crate) const fn has_layout<T>(size: usize, align: usize) -> bool {
    size_of::<T>() == size && align_of::<T>() == align
}

const _: () = assert!(
    has_layout::<ProjectileAabb>(48, 8)
        && has_layout::<ProjectileSpec>(40, 8)
        && has_layout::<EntityTarget>(88, 8)
        && has_layout::<ProjectileState>(376, 8),
    "projectile state types must keep expected size and alignment"
);
