use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::query::{Added, Changed, Or, With};
use bevy_ecs::system::{Local, Populated};
use mcrs_minecraft_engine::player_state::PlayerFileState;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_entity_attribute::attribute_keys::Attribute;
use mcrs_minecraft_entity_attribute::{
    AttributeInstances, InstanceChange, InstanceEdit, InstanceModifier, ModifierKind,
    ModifierOperation,
};
use mcrs_minecraft_env_damage as env_damage;
use mcrs_minecraft_resource::{ResourceLocationBuf, rl};

pub(crate) struct PowderSnowModifierKey(ResourceLocationBuf);

const _: () = {
    assert!(
        size_of::<PowderSnowModifierKey>() == 24,
        "PowderSnowModifierKey size must match ResourceLocationBuf"
    );
    assert!(
        align_of::<PowderSnowModifierKey>() == 8,
        "PowderSnowModifierKey alignment must match pointer alignment"
    );
};

impl Default for PowderSnowModifierKey {
    fn default() -> Self {
        Self(rl!("minecraft:powder_snow").to_buf())
    }
}

#[expect(
    clippy::type_complexity,
    reason = "Player attributes need update when frozen ticks change or attributes appear"
)]
pub(crate) fn sync_powder_snow_speed(
    mut players: Populated<
        (&PlayerFileState, &mut AttributeInstances),
        (
            With<Player>,
            Or<(Changed<PlayerFileState>, Added<AttributeInstances>)>,
        ),
    >,
    modifier_key: Local<PowderSnowModifierKey>,
) {
    for (file_state, mut instances) in &mut players {
        let frozen_ticks = file_state.ticks_frozen.max(0);
        let desired_amount =
            (frozen_ticks > 0).then(|| env_damage::powder_snow_speed_modifier(frozen_ticks));
        let current_amount = instances
            .set()
            .instance(Attribute::MovementSpeed)
            .and_then(|instance| {
                instance
                    .modifiers()
                    .iter()
                    .find(|modifier| modifier.id() == modifier_key.0.as_location())
            })
            .map(|modifier| modifier.amount());
        if desired_amount.map(f64::to_bits) == current_amount.map(f64::to_bits) {
            continue;
        }
        let edit = match desired_amount {
            Some(amount) => InstanceEdit::Put(InstanceModifier::new(
                modifier_key.0.clone(),
                amount,
                ModifierOperation::AddValue,
                ModifierKind::Transient,
            )),
            None => InstanceEdit::Remove(modifier_key.0.clone()),
        };
        if instances
            .bypass_change_detection()
            .apply(Attribute::MovementSpeed, &edit)
            == InstanceChange::Changed
        {
            instances.set_changed();
        }
    }
}
