use std::sync::Arc;

use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::query::With;
use bevy_ecs::system::{Local, Query, SystemParam};
use mcrs_engine_muxer::{TileKey, View};
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketTarget};
use mcrs_minecraft_engine::view_radius::VIEW_RADIUS;
use mcrs_minecraft_engine_registries::RegistryNames;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_item_stack::{NameLookup, NameTable, SoundEventRef, UseAnimation};
use mcrs_minecraft_protocol_game::clientbound::ClientboundSound;
use mcrs_minecraft_registries::{CurrentRegistries, Ref, Wire};
use mcrs_minecraft_sound::Sound;
use mcrs_minecraft_sound::sound_event_keys::SoundEvent;
use smallvec::SmallVec;

pub(crate) type ListenerQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Owner, &'static Transform), With<Player>>;

const DEFAULT_SOUND_RANGE: f64 = 16.0;
const BASE_CONSUME_PITCH: f32 = 0.8;
const PITCH_STEP_SCALE: f32 = 0.1;
const PITCH_MODULUS: u8 = 5;

#[derive(SystemParam)]
pub(crate) struct SoundSink<'w, 's> {
    listeners: ListenerQuery<'w, 's>,
    pub(crate) packets: MessageWriter<'w, OutboundPlayerPacket>,
    seed_sequence: Local<'s, u64>,
}

impl SoundSink<'_, '_> {
    pub(crate) fn next_seed(&mut self, actor: Entity) -> i64 {
        *self.seed_sequence = self.seed_sequence.wrapping_add(1);
        let mixed = self.seed_sequence.wrapping_add(0x517c_c1b7_2722_0a95) ^ actor.to_bits();
        ((mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9) >> 1) as i64
    }

    pub(crate) fn emit(
        &mut self,
        actor: Entity,
        actor_transform: &Transform,
        resolved: (Sound<Wire>, f64),
        volume: f32,
        pitch: f32,
    ) {
        let seed = self.next_seed(actor);
        let (sound_event, range) = resolved;
        let recipients = resolve_viewer_recipients(actor, actor_transform, range, &self.listeners);
        if recipients.is_empty() {
            return;
        }
        let position = actor_transform.translation;
        self.packets.write(OutboundPlayerPacket::sound(
            PacketTarget::PlayerSet(recipients),
            ClientboundSound {
                sound_event,
                source: ClientboundSound::SOURCE_PLAYERS,
                x: ClientboundSound::eighths(position.x),
                y: ClientboundSound::eighths(position.y),
                z: ClientboundSound::eighths(position.z),
                volume,
                pitch,
                seed,
            },
        ));
    }

    pub(crate) fn emit_neutral(
        &mut self,
        actor: Entity,
        actor_transform: &Transform,
        resolved: (Sound<Wire>, f64),
        volume: f32,
    ) {
        self.emit(actor, actor_transform, resolved, volume, 1.0);
    }
}

#[must_use]
pub(crate) fn consume_tick_pitch(step: u8) -> f32 {
    BASE_CONSUME_PITCH + f32::from(step % PITCH_MODULUS) * PITCH_STEP_SCALE
}

#[must_use]
pub(crate) fn default_consume_sound(animation: UseAnimation) -> SoundEvent {
    match animation {
        UseAnimation::Drink => SoundEvent::EntityGenericDrink,
        _ => SoundEvent::EntityGenericEat,
    }
}

pub(crate) fn resolve_sound(
    sound_ref: Option<&SoundEventRef>,
    fallback: SoundEvent,
    names: &RegistryNames<'_>,
    registries: &CurrentRegistries,
) -> Option<(Sound<Wire>, f64)> {
    let location = match sound_ref {
        Some(SoundEventRef::Inline(inline)) => {
            return Some((
                Ref::Inline(Arc::new(inline.clone())),
                f64::from(inline.range.unwrap_or(16.0)),
            ));
        }
        Some(SoundEventRef::Reference(id)) => names.name(NameTable::SoundEvent, id.0)?,
        None => fallback.location(),
    };
    let found = registries.entries::<SoundEvent>()?.by_name(location)?;
    Some((Ref::Id(found.into()), DEFAULT_SOUND_RANGE))
}

fn resolve_viewer_recipients(
    actor: Entity,
    actor_transform: &Transform,
    range: f64,
    listeners: &ListenerQuery<'_, '_>,
) -> SmallVec<[PlayerSession; 8]> {
    let actor_position = actor_transform.translation;
    let range_squared = range * range;
    let actor_tile = TileKey::at(actor_position);
    let mut recipients = SmallVec::new();
    for (viewer_entity, viewer_owner, viewer_transform) in listeners {
        if viewer_entity == actor {
            continue;
        }
        let viewer_position = viewer_transform.translation;
        let in_view = View {
            center: TileKey::at(viewer_position),
            radius: VIEW_RADIUS,
            subject: None,
        }
        .contains(actor_tile);
        if in_view && viewer_position.distance_squared(actor_position) < range_squared {
            recipients.push(viewer_owner.0);
        }
    }
    recipients
}
