use bytes::Bytes;
use mcrs_engine_muxer::EntityId;
use mcrs_minecraft_protocol::Result;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::Hand;
use mcrs_minecraft_protocol_game::SwingAnimationKind;
use mcrs_minecraft_protocol_game::clientbound::ClientboundDamageEvent;
use mcrs_minecraft_protocol_game::clientbound::ClientboundEntityEvent;
use mcrs_minecraft_protocol_game::clientbound::ClientboundSwingAnimation;
use mcrs_minecraft_protocol_game::clientbound::ClientboundTakeItemEntity;

use crate::encode::record;
use crate::wire_id::wire_id;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MobSignal {
    Hurt {
        damage_type: u32,
    },
    Died,
    Removed,
    EntityEvent {
        status: i8,
    },
    Taken {
        collector: EntityId,
        amount: i32,
    },
    Swing {
        hand: Hand,
        animation: SwingAnimationKind,
        duration: i32,
    },
}

impl MobSignal {
    pub(crate) fn encode(self, subject: EntityId) -> Result<Option<Bytes>> {
        let entity_id = wire_id(subject);
        let event = |entity_status| {
            record(&ClientboundEntityEvent {
                entity_id: entity_id.0,
                entity_status,
            })
        };
        let packet = match self {
            Self::Hurt { damage_type } => {
                let Ok(damage_type) = i32::try_from(damage_type) else {
                    tracing::warn!("the damage type {damage_type} of a hit has no id on the wire");
                    return Ok(None);
                };
                record(&ClientboundDamageEvent {
                    entity_id,
                    damage_type: VarInt(damage_type),
                    cause_entity_id: VarInt(0),
                    direct_entity_id: VarInt(0),
                    source_position: None,
                })?
            }
            Self::Died => event(ClientboundEntityEvent::DIED)?,
            Self::Removed => event(ClientboundEntityEvent::REMOVED)?,
            Self::EntityEvent { status } => event(status)?,
            Self::Taken { collector, amount } => record(&ClientboundTakeItemEntity {
                item_id: entity_id,
                player_id: wire_id(collector),
                amount: VarInt(amount),
            })?,
            Self::Swing {
                hand,
                animation,
                duration,
            } => record(&ClientboundSwingAnimation {
                entity_id,
                hand,
                animation,
                duration: VarInt(duration),
            })?,
        };
        Ok(Some(packet))
    }
}
