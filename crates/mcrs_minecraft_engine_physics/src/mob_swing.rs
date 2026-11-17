use bevy_ecs::resource::Resource;
use mcrs_minecraft_entity_key::EntityId;
use mcrs_minecraft_entity_rows::{Placed, RowStore};
use mcrs_minecraft_swing::{SwingModifiers, SwingSpec, SwingTracker};
use rustc_hash::FxHashMap;

use crate::mob_life::MobChanges;
use crate::mobs::Mobs;

#[derive(Resource, Debug, Default)]
pub struct MobSwingStates(pub FxHashMap<EntityId, SwingTracker>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwungMob {
    pub at: Placed,
    pub spec: SwingSpec,
}

pub fn swing_mob<M>(
    rows: &RowStore<M>,
    mobs: &Mobs,
    states: &mut MobSwingStates,
    changes: &mut MobChanges,
    at: Placed,
    spec: SwingSpec,
    tick: u64,
) -> bool {
    if !mobs.0.get(&at.id).is_some_and(|mob| mob.kind.is_living()) {
        return false;
    }
    if !rows
        .state_of(at)
        .is_some_and(|state| state.living.is_some())
    {
        return false;
    }
    let base_duration = spec.duration.max(0);
    let tracker = states.0.entry(at.id).or_default();
    if tracker
        .try_start(tick, base_duration, SwingModifiers::default())
        .is_none()
    {
        return false;
    }
    changes.swings.push(SwungMob {
        at,
        spec: SwingSpec {
            duration: base_duration,
            ..spec
        },
    });
    changes.touched.push(at);
    true
}
